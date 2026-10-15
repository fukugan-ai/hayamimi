# R8v step 1c: the second held-out run

One run per set, after the council fixed the limits (the mid-sentence rate in the RP-66 row, the sorting agreement in 3.17 count 9) and before this result was seen. B is `min_silence` 0.12 s only. The threshold, the completeness table and the sorting rules are those of steps 1 and 1b and did not change.

    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_heldout2_ja_probe
    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_heldout2_en_probe

Both were started as `sudo -n nice -n -5 <test binary> ...`.

## Sets

- **ja:** the first 80 rows of ReazonSpeech `test` (`japanese-asr/ja_asr.reazonspeech_test`, read through the Hugging Face datasets server) that last 2 s to 15 s, in row order, after leaving out the 15 rows whose sentence is the same as one of the 15 references of `testdata/eval_real` ja (compared without 、 。 ? ! and spaces). The audio was converted to 16 kHz mono 16-bit, joined with 1.5 s of silence by `hayamimi-eval fetch-set --concat` into `testdata/r8v_heldout2/ja.wav` and `ja.json` (outside git); `ja-rows.json` there has the row index, the reference and the sha256 of each wav. The rows used are 1, 3, 4 and on, up to the 80th that fit the length.
- **en:** the AMI meetings of `testdata/eval_diar` with the same turn rule as step 1 (one speaker, 1 s or more, no other speaker within 0.3 s), every turn after the first 12 of each meeting: 46 turns (9, 19, 4, 6, 8 per meeting).

References of the ja set: 80; with 。 inside the sentence (a 。 that is not the last character): **1**; with 、: 17; ending in 。: 61.

## Result, ja (80 rows)

| item | value |
|---|---|
| utterances with no segment of A | 0 |
| segments of A | 91 |
| candidates / with voice going on / complete / complete with voice going on | 105 / 25 / 49 / 5 |
| splits by A | 11 |
| splits by B alone | 5 (real boundary 0, tail noise 1, mid-sentence 4) |
| splits in total with B | 16 |
| delay, last final, p50 / p95 | 617 / 1246 ms (n 80) |
| delay, A alone, p50 / p95 | 906 / 1362 ms |
| vad_ms per window, A plus B, p50 / p95 | 0.300 / 0.483 ms |

### The two limits

- **Mid-sentence rate:** 4 of 80 utterances (5.0 %) against the limit of 10 % (8 pass, 9 fail): **passed.**
- **Sorting agreement:** the sortings differ on 1 of 5 splits. The limit is 0 differences when there are under 20 splits: **not met.**
- vad_ms p95 of A plus B is 0.483 ms against 2 ms: passed.

The one split on which the sortings differ is a row whose reference ("これは。") is much shorter than what is spoken: the final before the line is "こんなのどこで覚えたんですか？", the next is "フランス人は。". By the reference the cut is mid-sentence, by the final of A the next sentence is tail noise. The other four splits are mid-sentence in both sortings, and one tail noise in both.

The mid-sentence splits are cuts at a 。 the recognizer put inside what the reference writes as one sentence: "お互い。きっかけがなかったんだ。" for "おたがいあやまるきっかけがなかったんだね。", "この言い分かる。" for "…コウモリの言い分ということでしたけれども。", and "罰金自体が…思いますし。" cut from one sentence of the reference.

### The ja limit of RP-66 on this set

With the last-final delays (3.17 count 10): P_B = 1246 ms, P_A = 1362 ms, so 1246 + (1341 - 1362) = 1225 ms, rounded up to 10 ms: **1230 ms**. It is below 1341 ms. The formula takes the figure from `lat-ja` (1050 ms, `probe.md`); the three sets give 1050, 960 and 1230 ms, so the limit moves by 270 ms between sets and the formula's input is not stable.

## Result, en (AMI, 46 turns, recorded only)

| item | value |
|---|---|
| segments of A | 67 |
| splits by A | 27 |
| splits by B alone | 14 (real boundary 10, tail noise 2, mid-sentence 2) |
| splits in total with B | 41 |
| mid-sentence per utterance | 2 of 46 |

Per meeting the delays (last final p50 / p95, A alone p50 / p95) are ES2011a 306 / 7886 and 492 / 8334 ms, IS1008a 2566 / 13694 and 2914 / 13694 ms, ES2004a 190 / 308 and 308 / 582 ms, IS1009a 504 / 5442 and 902 / 5442 ms, TS3003a 370 / 730 and 506 / 1082 ms. The human-set end of an AMI turn is not the 20 ms level rule of `lat-en`, so these are a record. vad_ms p95 of A plus B is 0.465 to 0.717 ms per meeting. The en sorting rests on the final of A (3.17 count 9), so it is a reference figure and does not judge.

## The judgement

B at 0.12 s keeps the mid-sentence rate under its limit (5.0 % against 10 %) and does not meet the sorting-agreement limit (1 difference where 0 is allowed). The design says that a limit crossed by 0.12 s sends the work back to the council before step 2, so step 2 does not start on this result.

## 残る穴

- **The agreement limit rests on 5 splits.** With under 20 splits the limit is 0 differences, so one reference that is shorter than the speech decides it. The 5 splits are too few to say whether the difference is the early path's or the reference's (NOT VERIFIED).
- **The ja limit of RP-66 is not stable** (1050, 960, 1230 ms on three sets).
- **Eight of 80 ja rows is the limit; 4 were found.** The set has one reference with a 。 inside a sentence, so the sorting by the reference's 。 had almost no reference sentence ends other than the final one to find.
- **AMI turns:** the 46 are every turn left after step 1; their en delays are not judged.

## 採らない手段

- Moving the limits or the table after seeing the result: not done.
- Reading the agreement limit as met because the mid-sentence rate passed: the two limits are separate.
- Using the en sorting for a pass or fail: it rests on the final of A.
