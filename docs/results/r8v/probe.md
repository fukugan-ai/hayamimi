# R8v step 1: offline probe of the early end-of-speech detector

The probe is `native/hayamimi-asr/tests/r8v_probe.rs` (`#[ignore]`, offline). It changes no product code and no default:

    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture

The raw output is the `R8V` lines (15) and the `CAND` lines (51) of that run.

## Conditions (C1 to C4)

- Input: `testdata/lat-ja.wav` (23 utterances) and `testdata/lat-en.wav` (15), with the manifests under `testdata/manifests/`.
- Current detector A: threshold 0.5, min_silence 0.30 s, max_speech 12 s, shortest speech 0.25 s (the production settings).
- Early detector B: threshold 0.65 (its end line is 0.50), min_silence 0.12 s, max_speech 12 s, shortest speech 0.25 s.
- A and B are fed the same 512-sample windows in the same order. A notice of B is a **candidate** only when A is inside speech at that window (`detected()`). The candidate sample is the end of that window.
- Each candidate is solved from the voice start of the enclosing A segment, minus the 1.0 s pre-roll, up to the candidate sample, through `Engine::transcribe` (recognition, retry, ITN, punctuation, dictionary; speaker stage off; a new engine per candidate with `Policy::single(lang)`). A slice that yields several finals is joined.
- The completeness table is in the probe as constants (`JA_FINAL`, `JA_CONNECTIVE`, `EN_CONTINUATION`), in a form that moves to `endpoint.rs`. A ja text is complete when it ends in 。？！, the form before that is a sentence ending, and it is not a connective. An en text is complete when it ends in . ? ! and its last word is not a continuation word. The rows are an initial table; the test designer fixes them in step 2.
- A candidate is **inside an utterance window** when the manifest utterance started before it and the voice continues 300 ms or more after it (`end_sample - candidate >= 4800`).
- Percentiles are nearest rank (`hayamimi_eval::percentile::nearest_rank`). vad_ms is the time of `accept_waveform` plus the segment take per window, for A alone and for A plus B, over both wavs (9224 windows).

## Result

| item | ja | en |
|---|---|---|
| utterances | 23 | 15 |
| B notices (all inside A's speech) | 35 | 22 |
| raw candidates inside an utterance window (before judging) | 16 | 7 |
| candidates judged complete | 21 | 18 |
| complete candidates inside an utterance window | **6** | **5** |
| utterances that got a complete candidate | 18 of 23 | 14 of 15 |
| end diff of A (segment end - manifest end) p50 / p95 | 524 / 877 ms | 78 / 250 ms |
| end diff of B (last B segment end - manifest end) p50 / p95 | 262 / 705 ms | 64 / 238 ms |
| detection - manifest end, last B notice, p50 / p95 | 382 / 825 ms | 184 / 358 ms |
| detection - manifest end, first complete candidate, p50 / p95 | 216 / 880 ms | 161 / 341 ms |

| vad_ms per window (both wavs, 9224 windows) | p50 | p95 | limit |
|---|---|---|---|
| A alone | 0.133 ms | 0.241 ms | |
| A plus B | 0.263 ms | 0.472 ms | p95 2 ms |

Detection delay is the window end of the candidate minus the manifest `end_sample`. It does not contain the solve time of the candidate (recognition, ITN, punctuation), which step 2 adds as `L_dec`.

## The three stop conditions of R8v

1. ja end-diff p50 not shortened by 150 ms or more from 524 ms: **not hit.** B's end is 262 ms (gain 262 ms). The first complete candidate is seen 216 ms after the manifest end at p50 (gain 308 ms against A's 524 ms).
2. A complete-judged candidate inside an utterance window: **hit.** ja 6, en 5, listed below. Step 2 does not start on this result; it goes to the council.
3. vad_ms: replaced by the p95 limit of 2 ms on A plus B. **Not hit:** p95 is 0.472 ms.

The stop on a zero-candidate result did not apply: ja has 35 candidates and en 22.

### Complete candidates inside an utterance window

Each is a sentence boundary in the middle of one manifest utterance (the utterance holds more than one sentence). Cutting there splits one utterance into two finals. Whether that is acceptable is the council's decision.

| lang | candidate sample | text |
|---|---|---|
| ja | 670208 | アピヤはサモアの首都です。 (the next sentence follows in the same utterance) |
| ja | 1930752 | でも駿河さん91歳。力強い走りに変わっていきます。 |
| ja | 2007040 | ずいぶんありましたけどね。 |
| ja | 2611200 | 10万人以上の戦死者を出していた。 |
| ja | 2761216 | 言ったろう。 (the next sentence follows) |
| ja | 2871808 | 高校生200人に聞きました。 |
| en | 34304 | He was in a fevered state of mind. |
| en | 210432 | Demand. Or there would be trouble. |
| en | 643584 | He would see how things turned out tomorrow, and then he would talk to her. |
| en | 1213440 | He would get one today. It would probably be on his desk when he got back. |
| en | 1600000 | His first impulse was to write but four words in reply. |

The en candidate at 1600000 is followed by one at 1618432 whose text is longer ("..., Go to the Devil."): a complete judgement at the first one would cut the sentence short.

## 手番 2 の前に要る物

- **No held-out set.** 0.65 and 0.12 were chosen on the same `lat-ja` and `lat-en` that this probe measures. The numbers above are fitted, not tested. Step 2 needs a recording that took no part in the choice before a number from it is read as a result.
- **`Final.speaker_audio` comes from the segment.** It is taken from `segment.samples` in `process` (`native/hayamimi-asr/src/pipeline.rs:680`). If the early path discards the segment, the source of that audio is lost and the speaker stage gets nothing for that final. Step 2 keeps the segment's samples when it finalizes early, or cuts the same samples at the candidate.
- **The solved slice approximates `process`.** The probe feeds the slice to a fresh engine, whose own detector may cut the slice again. A production early path hands one `Segment` to `process`, so the text may differ in edge cases.
- **A reused engine aborted inside the detector's buffer.** The 7th `transcribe` on one engine aborted (`circular-buffer.cc` `Get` and `Pop`, `Invalid n: -2752. size: 2048`). The probe opens an engine per candidate. The cause is not isolated (NOT VERIFIED: the detector's timeline after `finish` is the suspect); step 2 must not reuse the engine's detector across slices without a test for it.
- **Not measured here:** `L`, which is not part of pass or fail (RP-66 judges `d` and records `L`, decode and translation p95). The council's estimate of ja `d` p95 of about 1031 ms against the RP-66 limit of 1050 ms (a margin of about 19 ms) is not re-measured. The ja detection delay p95 is 880 ms before the solve time is added.

## 採らない手段

- Changing `THRESHOLD`, `min_silence` or `max_speech` of the production detector: not done; the probe builds its own detectors.
- Putting `L` into the pass or fail of this step: RP-66 judges only `d`.
- A "2x" comparison for vad_ms: replaced by the p95 limit of 2 ms.
- Reusing one engine for all candidates: it aborted, see above.
- Touching `vad.rs`, `pipeline.rs`, `args.rs` or `session.rs`: not touched.
