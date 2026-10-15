# R8v step 1b: the splits by B alone, sorted against the reference

No new audio. The probe (`native/hayamimi-asr/tests/r8v_probe.rs`) is run again on the sets of step 1 and prints the text of both finals of each split by B alone, so the splits can be sorted by the rules of 3.17 (counts 8 to 10). Thresholds, completeness table, B's `min_silence` of 0.12 s and the split counts are the ones of step 1; they did not change (the splits by B alone are the same 4, 2, 9 and 31).

    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_latency_set_probe
    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_heldout_ja_probe
    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_heldout_en_probe
    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_latency_set_probe_min_silence_020

## How a split is sorted

A split is the line between two finals that the early path makes out of one segment of A. The final before the line (with every final before it) is aligned to the reference by minimum edit distance (ja: NFKC characters without punctuation and spaces; en: lower-case words without punctuation). Then, in this order:

1. **Tail noise:** the final after the line is empty, or none of its characters (words) is in the rest of the reference.
2. **Real sentence boundary:** a sentence end of the reference lies within one unit (character or word) of where the alignment ends. ja sentence ends are 。 ? !.
3. **Mid-sentence:** neither.

For ja the sorting is done twice: against the `ref` of the manifest, and against the final of A for the same segment (its 。). For en, no punctuated reference exists. `lat-en` has an unpunctuated `ref`, and the AMI meetings of `testdata/eval_diar` have an RTTM and no words at all (checked). So en is sorted against the final of A only, which is the `ASSUMPTION` of 3.17 count 9, and the en figures below are not independent of the early path: the candidate was judged complete because its text ended in . ? !, and the final of A is made by the same recognizer.

## Result

| set | splits by B alone | real boundary | tail noise | mid-sentence | mid per split | mid per utterance |
|---|---|---|---|---|---|---|
| lat-ja (by reference) | 4 | 1 | 2 | 1 | 1 of 4 | 1 of 23 |
| held-out ja, 81 clips (by reference) | 9 | 4 | 0 | 5 | 5 of 9 | 5 of 81 |
| ja together | 13 | 5 | 2 | 6 | 6 of 13 | 6 of 104 (5.8 %) |
| lat-en (by the final of A) | 2 | 1 | 0 | 1 | 1 of 2 | 1 of 15 |
| held-out en, AMI 60 turns (by the final of A) | 31 | 28 | 2 | 1 | 1 of 31 | 1 of 60 |
| en together | 33 | 29 | 2 | 2 | 2 of 33 | 2 of 75 (2.7 %) |

The pass line of 3.17 is the rate of mid-sentence cuts. The council fixed it after this result: the denominator is the utterances (all the samples of the frame), the numerator the ja splits that the reference sorts as mid-sentence, and the limit is ja 10 %; en is recorded and judges nothing. On these two earlier sets the ja figure is 6 of 104 (5.8 %), which is under the limit; step 1c is the judgement (`heldout2.md`).

The ja sortings, by reference against by the final of A: **agree 4, differ 9 of 13** (the sortings agree on 1 of 4 in lat-ja and on 3 of 9 in held-out ja). The council later fixed the limit at 5 % of the splits that differ (0 when under 20 splits); this record is far above it.

The mid-sentence ja cases: in the held-out set, three are inside the one reference sentence "…気を付けてください（縮みの原因になったり、…）。" (the reference has its 。 only at the very end, and the cut is at the full stop the recognizer put before the parenthesis), and two are cuts at "…選手やクラブが崩れる。" and "…七つのウィケットを失い。" inside one reference sentence; in `lat-ja`, one is the cut at "言ったろう。" inside "言ったろヒーローよりも…". The tail noise cases (both `lat-ja`) are a spurious "さあ。" and a garbled continuation after the end of the reference. The en mid case of lat-en is "He was in a fevered state of mind." cut from a sentence that the final of A joins with a comma.

## Delay by the last final (count 10)

The delay is taken at the last final of the utterance, including the sentence after a split. Utterances with no segment of A are left out (lat-ja 1, held-out ja 8; the same utterances as step 1).

| set | last final p50 / p95 | A alone p50 / p95 |
|---|---|---|
| lat-ja (n 22) | 508 / 880 ms | 824 / 1177 ms |
| lat-en (n 15) | 186 / 550 ms | 378 / 550 ms |
| held-out ja (n 73) | 428 / 1340 ms | 532 / 1724 ms |
| AMI, per meeting | recorded in the run output only | |

These are the figures of step 1 ("last final" rows); the ja limit by the formula of count 5 with the last-final figures is 880 + (1341 - 1177) = 1044, rounded up to 1050 ms (lat-ja), and 1340 + (1341 - 1724) = 957, rounded up to 960 ms (held-out ja), the same as before.

## B with min_silence 0.20 s, latency sets only (a record)

| set | splits by B alone | mid | last final p50 / p95 | vad_ms A plus B p95 |
|---|---|---|---|---|
| lat-ja, 0.12 s | 4 | 1 | 508 / 880 ms | 0.477 ms |
| lat-ja, 0.20 s | 4 | 1 | 654 / 965 ms | 0.408 ms |
| lat-en, 0.12 s | 2 | 1 | 186 / 550 ms | 0.477 ms |
| lat-en, 0.20 s | 1 | 0 | 282 / 550 ms | 0.511 ms |

At 0.20 s the splits by B alone do not fall in ja (4 and 4) and fall by one in en; the delay grows by 146 ms (ja p50) and 96 ms (en p50). The council then fixed step 1c to 0.12 s only, so this record stays a record.

## 残る穴

- **Translation chrF not taken.** The council dropped it from step 1c; the RP-66 run (step 3) records it and judges nothing with it.
- **The ja sortings differ in 9 of 13.** Most differences come from the recognizer's own punctuation and misrecognitions in the final of A (for example a dropped first sentence), not from the early path. This run does not separate the two (NOT VERIFIED).
- **en has no independent reference.** See above.
- **Step 1c** ran after the limits were fixed; its result is in `heldout2.md`.

## 採らない手段

- Reading "no splits by B alone" as the pass line: the design replaced it with the rate of mid-sentence cuts.
- Using the en sorting for a pass or fail: it rests on the final of A.
- Sorting by hand: all sorting is done by the code of the probe, with unit tests for the alignment and the three kinds.
