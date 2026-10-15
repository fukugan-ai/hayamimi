# R8v step 1: held-out run of the early end-of-speech detector

One run per set, after the thresholds, the completeness table and the pass line were fixed (`probe.md`, `native/hayamimi-asr/tests/r8v_probe.rs`). Nothing was moved after the result was seen.

    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_heldout_ja_probe
    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_heldout_en_probe

## Sets

- **ja:** the 100 clips of `testdata/fleurs_bench/ja` (`manifest.json`) without the 19 clips whose `id` is one of the 8 ids of the `lat-ja` golden clips (1669, 1674, 1677, 1679, 1684, 1702, 1705, 1710; several clips share an id). 81 clips, joined with 1.5 s of silence by `hayamimi-eval fetch-set --pin` and `--concat` into `testdata/r8v_heldout/ja.wav` and `ja.json` (outside git). The end of an utterance is the end of its speech by the 20 ms level rule of the join.
- **en:** the AMI meetings of `testdata/eval_diar` (5 meetings of 10 minutes). An utterance is a turn of one speaker in the RTTM that lasts 1 s or more and has no other speaker's turn within 0.3 s before or after it. The first 12 such turns of each meeting are taken (60 utterances), so that the run fits its time; the cap was set before the run. The end of a turn is a human-set boundary, so the en delay is recorded and not judged; the judgement is the split count.

## Result

| item | ja (81 clips) | en (60 turns, 5 meetings) |
|---|---|---|
| utterances with no segment of A | 8 | 2 |
| segments of A | 91 | 92 |
| candidates (B notices inside A's speech) | 151 | 132 |
| candidates with voice going on after them | 67 | 63 |
| candidates judged complete | 79 | 74 |
| complete candidates with voice going on | 9 | 34 |
| splits by A | 18 | 41 |
| **splits by B alone** | **9** | **31** |
| splits in total with B | 27 | 72 |
| delay, first complete or A, p50 / p95 | 228 / 1340 ms (n 73) | recorded per meeting, below |
| delay, A alone, p50 / p95 | 532 / 1724 ms (n 73) | recorded per meeting, below |
| vad_ms per window, A alone, p50 / p95 | 0.130 / 0.224 ms | 0.127 to 0.190 / 0.231 to 0.333 ms |
| vad_ms per window, A plus B, p50 / p95 | 0.258 / 0.438 ms | 0.253 to 0.370 / 0.452 to 0.652 ms |

en per meeting (A segments, splits by A, splits by B alone, delay first complete or A p50 / p95, delay A alone p50 / p95):

| meeting | A segments | by A | by B alone | first complete or A | A alone |
|---|---|---|---|---|---|
| ES2011a | 23 | 12 | 6 | -60 / 832 ms (n 11) | 776 / 10120 ms |
| IS1008a | 9 | 0 | 4 | 428 / 7574 ms (n 12) | 1282 / 7990 ms |
| ES2004a | 19 | 9 | 8 | -3016 / 1030 ms (n 12) | 462 / 7626 ms |
| IS1009a | 12 | 3 | 6 | 20 / 1862 ms (n 11) | 756 / 7456 ms |
| TS3003a | 29 | 17 | 7 | -1560 / 2144 ms (n 12) | 918 / 2144 ms |

A negative delay is a first complete candidate made before the human-set end of the turn, that is, inside it. The large p95 of "A alone" in the meetings may come from a segment of A that holds several speakers' speech, so that its detection comes long after the end of the single turn taken (NOT VERIFIED: the run does not print which segment it was).

## The pass line

The line: splits by B alone are 0, and the splits in total with B are not more than the splits by A.

- ja: **not passed.** Splits by B alone are 9; the total with B is 27 against 18 by A.
- en: **not passed.** Splits by B alone are 31 and every meeting has some (6, 4, 8, 6, 7); the total with B is 72 against 41 by A.
- vad_ms p95 of A plus B is 0.438 ms (ja) and at most 0.652 ms (en) against the limit of 2 ms: passed.

The lat sets (`probe.md`) did not pass either. The line fails on the held-out sets too, so the failure does not come from the thresholds being fitted to `lat-ja` and `lat-en`. Step 2 does not start; the result goes to the council.

## The ja limit on the held-out set

With the formula of 3.17 (count 5) on this set: P_B = 1340 ms, P_A = 1724 ms, so 1340 + (1341 - 1724) = 957 ms, rounded up to 10 ms: **960 ms**. On `lat-ja` the same formula gives 1050 ms (`probe.md`). Both are below 1341 ms. The two differ by 90 ms, so the limit is not stable between the sets; the formula was fixed to be taken from `lat-ja`.

## 残る穴

- 8 of the 81 ja clips and 2 of the 60 AMI turns have no segment of A touching them; they are left out of the delay figures. Why A misses them is not shown by this run (NOT VERIFIED).
- The AMI turns are the first 12 of each meeting; the later part of each 10-minute excerpt is not run.
- The remainder rule (a remainder under 0.5 s is not solved) can under-count the splits by B alone.

## 採らない手段

- Moving the thresholds, the table or the pass line after the result: not done.
- Judging the en delay: the human-set end of an AMI turn is not the 20 ms level rule of `lat-en`.
- Running the held-out sets more than once: one run each.
