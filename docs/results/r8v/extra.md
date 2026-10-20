# R8x-5: the offline gate of C and D against the current rule

The gate replays the early path twice over the same audio and the same detector pass. The arm W is the current rule (A and B only, complete candidates confirmed at once). The arm X is W with two more early detectors: C (threshold 0.80, end line 0.65, silence 0.12 s) and D (threshold 0.50, end line 0.35, silence 0.12 s). Policy B (`--early-complete nouns`) is in neither arm. The probes are in `native/hayamimi-asr/tests/r8v_probe.rs`:

    cd native && cargo test --release -j 2 -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_latency_set_probe_extra_notices
    cd native && cargo test --release -j 2 -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_heldout_ja_probe_extra_notices

The outputs are `extra-lat.log` and `extra-heldout-ja.log` (one JSON row per set and arm, written by the probe) and the full printed output in `extra-lat.stdout.log` and `extra-heldout-ja.stdout.log`.

## Result

The gate does not pass. The pass line is that, on lat-ja and on held-out ja, both figures of X are within those of W.

| set | figure | W | X | within W |
|---|---|---|---|---|
| lat-ja (23 utterances) | candidates finalised with voice going on after them | 3 | 3 | yes |
| lat-ja | splits by the early path alone | 4 | 6 | no |
| held-out ja (81 clips) | candidates finalised with voice going on after them | 9 | 18 | no |
| held-out ja | splits by the early path alone | 9 | 26 | no |
| lat-en (15 utterances, recorded, judges nothing) | the same two figures | 2 and 2 | 2 and 2 | yes |

## Controls

- Positive control: the arm W gives the live figures (lat-ja 3 and 4, held-out ja 9 and 9), and the candidate counts of `shadow.md` (35 and 151). A W off these figures fails the probe before X is compared.
- Machinery control: on lat-ja, the replay of X over a pass without C and D has the same figures and the same candidates as W (`R8X lat-ja MACHINERY_OFF_EQUALS_W true`). It was not run on the held-out set.
- Negative controls (unit tests): a W off the live figures fails before X is compared; removing the voice condition from the D term makes the +1 / +0 fixture fail.

## How the first figure counts D

The first figure is the number of candidates finalised with voice going on after them inside the same segment of A. It is the complete candidates (whichever notice finalised them, B or C) plus the held candidates that D finalised and that voice went on after. A held candidate that D finalised with no voice after it adds 0, and a held candidate D did not finalise adds 0. Voice going on is read from the notices of B only, as in `voice_continues`.

The second figure is the finals beyond the first that the early path makes out of one segment of A, over every cause (B, C and D).

The unit tests `a_held_candidate_d_finalises_adds_one_with_voice_going_on_and_none_without` (+1 with voice after, +0 without, +0 for a held candidate D did not finalise) and `the_arm_w_has_no_term_of_d` fix this.

## Where the excess comes from

| set | X candidates | D notices that found a held candidate | finalised by D | of those, voice after | extra finals of X over W |
|---|---|---|---|---|---|
| lat-ja | 66 (W 35) | 8 | 3 | 0 | 2 (in 2 utterances) |
| held-out ja | 307 (W 151) | 41 | 12 | 8 | 17 (in 16 utterances) |

In held-out ja the first figure of X is 18: 10 complete candidates with voice after (W has 9) and 8 held candidates D finalised with voice after. The 29 notices of D that found a held candidate and did not finalise it all ended on a connective or a particle; none was stopped by a voice start of B.

Extra finals in utterances where D finalised nothing can only come from C. They are 2 of 2 in lat-ja (utterances 3 and 7) and 9 of 17 in held-out ja (8 utterances). The other 8 extra finals of held-out ja are in utterances where D finalised a held candidate.

The first figure does not see these C splits. In each of them the candidate that C finalised early has no stretch of voice of B starting after its end (`continues false`), because C ends a stretch inside a stretch that B still reads as one. The first figure of the complete candidates rises by 0 in lat-ja and by 1 in held-out ja, while C makes 2 and 9 extra finals. Only the second figure sees them. This count is by utterance and by the absence of a D finalisation; the probe does not log which detector's notice finalised a candidate, so "C cut inside a stretch of B" is read from the pair (extra final, no D finalisation, `continues false`) and is not logged per notice.

## Utterances 16 and 22 of lat-ja

The numbers count from 1, as the end-to-end records do (index 15 and 21 in the manifest). Delays are in ms after the end of the utterance. "A" is the delay of the main detector alone.

| utterance | held candidate | D | last final, W | last final, X | A |
|---|---|---|---|---|---|
| 16 | at 144.74 s, incomplete: "...体感はどうでしょうか？近藤さん。" | no notice of D found it held; a complete candidate at 144.96 s ("...体感はどうでしょうか？") replaced it | 590 | 590 | 1006 |
| 22 | at 187.58 s and again at 187.74 s, incomplete: "...TELASAで独占配信。" | D finalised it at 187.97 s; no voice after it | 891 | 699 | 891 |

D finalises the held candidate of utterance 22, 192 ms earlier than W. It does not act on utterance 16: its held candidate is replaced before a notice of D reads it, and the final of 16 stays at 590 ms in both arms. The other two lat-ja utterances where D finalised a held candidate are 18 and 19; both are far before the end of the utterance and have no voice after.

## Delay

Delay at the last final, p50 / p95 (ms):

| set | W | X |
|---|---|---|
| lat-ja (n 22) | 508 / 880 | 396 / 755 |
| held-out ja (n 73) | 428 / 1340 | 384 / 1212 |

These come from the replay, not from an end-to-end run.

## What the probe does not model

- A complete sentence shorter than 1000 ms is confirmed at once in both arms, as in the earlier probes; the live path holds it (`too_short`), and D could then finalise it. This does not move the difference between W and X.
- D reads a voice start of B at or after the end of the held range; the live state of the early path compares with the end of the stretch of voice (`floor`). The two differ by at most one window.
- The two tables of connective forms and particles that D checks are copied into the probe (`JA_OPEN_FORMS`), because `endpoint.rs` keeps them private. A unit test checks each copied form against `tail_of`; a form added to `endpoint.rs` later is not seen by it.
- The fixtures of T-RP-86-001 to 009 put W at 2 / 3 and 5 / 6, which the positive control (3 / 4 and 9 / 9) rejects; the tests of the probe use the live figures as W. The part of T-RP-86-001 and 008 that reads the end-to-end records belongs to `hayamimi-eval/tests/rp86.rs`.
- The en rows are a record; D does nothing in en.
