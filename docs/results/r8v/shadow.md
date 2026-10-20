# R8v offline gate: the shadow rule against the live rule

The gate runs the replay of `probe.md` twice on the same audio, once with the live completeness rule (`endpoint::is_complete`) and once with the shadow rule (a candidate is complete unless `endpoint::shadow_early_hold` answers `Some(Hold::Incomplete)`). The shadow rule keeps every sentence the live rule completes and adds a Japanese sentence under a closing mark that ends in a noun or an honorific. The probe is `native/hayamimi-asr/tests/r8v_probe.rs`:

    cd native && cargo test --release --jobs 2 -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_latency_set_probe_shadow_rule
    cd native && cargo test --release --jobs 2 -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_heldout_ja_probe_shadow_rule

The outputs are `shadow-lat.log` and `shadow-heldout-ja.log`. Each log holds the live-rule run first (names `lat-ja`, `lat-en`, `heldout-ja`), then the shadow-rule run (names ending in `-shadow`), then one `SHADOW_GATE` line.

## Result

The gate does not pass. Both Japanese sets exceed the live figures on both numbers.

The two numbers are the **complete candidates with voice going on** (a candidate judged complete while another stretch of voice follows inside the same segment of A) and the **splits by B alone** (finals beyond the first that the early path makes out of one segment of A).

| set | complete candidates with voice going on, live / shadow | splits by B alone, live / shadow | within the live figures |
|---|---|---|---|
| lat-ja (23 utterances) | 3 / 4 | 4 / 5 | no |
| held-out ja (81 clips) | 9 / 19 | 9 / 19 | no |
| lat-en (15 utterances, recorded) | 2 / 2 | 2 / 2 | yes (the rule is the same in en) |

The passing line is both numbers within the live figures on lat-ja and on held-out ja. It is not met, so the next step of 3.17 (the widened early end) does not start; the result goes to the council.

## Positive control

The live-rule run of the same probe reproduces the figures of the design in every column that the design states:

| set | candidates | with voice going on | complete with voice going on | splits by B alone |
|---|---|---|---|---|
| lat-ja, design | 35 | 9 | 3 | 4 |
| lat-ja, live run in `shadow-lat.log` | 35 | 9 | 3 | 4 |
| held-out ja, design | 151 | 67 | 9 | 9 |
| held-out ja, live run in `shadow-heldout-ja.log` | 151 | 67 | 9 | 9 |

## Negative control

With `shadow_complete` temporarily made the live rule (`is_complete`), the same two tests give shadow figures equal to the live ones: lat-ja 3 / 3 and 4 / 4, lat-en 2 / 2 and 2 / 2, held-out ja 9 / 9 and 9 / 9, with `within the live figures: true` on all three (2 tests passed). The difference in the table above therefore comes from the rule alone. The restored `r8v_probe.rs` has the same sha256 as before the change:

    d20664bde2a26693ed084aa8fd894853a52c928669869d889e3b16df7a832aae  native/hayamimi-asr/tests/r8v_probe.rs

## Other figures of the shadow run

| item | lat-ja live | lat-ja shadow | held-out ja live | held-out ja shadow |
|---|---|---|---|---|
| candidates judged complete | 22 | 27 | 79 | 90 |
| splits by A | 4 | 4 | 18 | 18 |
| delay at the last final, p50 / p95 (ms) | 508 / 880 | 472 / 825 | 428 / 1340 | 428 / 1340 |
| mid-sentence splits (sorted by the reference) | 1 of 23 utterances | 2 of 23 | 5 of 81 | 15 of 81 |

The delay of the held-out set does not move, so the shadow rule buys no latency there and adds 10 splits by B alone, 10 of them in the middle of a sentence.

The `ADDED` lines of the logs list the complete candidates with voice going on that the live rule rejects. After one such candidate the line moves, so the later candidates of the same segment differ between the two runs; the count of `ADDED` lines is therefore not the difference of the table (10 lines against +10 in held-out ja, 1 line against +1 in lat-ja).

## Candidates the shadow rule adds

The place is the index in the manifest, the time in the joined input, and the distance to the end of the utterance.

lat-ja (`testdata/lat-ja.wav`):

| utterance | clip | time | before the end | text |
|---|---|---|---|---|
| 12 | `testdata/eval_real/ja_05.wav` | 118.14 s | 3085 ms | でも駿河さん。91歳。 |

The reference of the utterance is でも敦賀さん、９１歳力強い走りに変わっていきます。; the cut falls after 91歳 and the rest follows as a second final.

held-out ja (`testdata/r8v_heldout/ja.wav`):

| utterance | clip | time | before the end | text |
|---|---|---|---|---|
| 1 | `fleurs_bench/ja/ja_002.wav` | 20.03 s | 8448 ms | 合金とは金。 |
| 15 | `fleurs_bench/ja/ja_019.wav` | 258.78 s | 3796 ms | 被害物取締法への資金提供を開始し。FBIAR。 |
| 21 | `fleurs_bench/ja/ja_028.wav` | 345.28 s | 2620 ms | 南アフリカにいるすべての国立公園と同様にこの数。 |
| 29 | `fleurs_bench/ja/ja_042.wav` | 473.02 s | 10236 ms | ここには当時を生き延びた恩薦。 |
| 31 | `fleurs_bench/ja/ja_046.wav` | 511.33 s | 6872 ms | 例えば世界で最も不可欠。 |
| 45 | `fleurs_bench/ja/ja_060.wav` | 720.35 s | 3548 ms | 日本の原子力庁によれば日本。 |
| 59 | `fleurs_bench/ja/ja_074.wav` | 923.01 s | 8652 ms | OTHERCONSENTSCHAGE。 |
| 59 | `fleurs_bench/ja/ja_074.wav` | 929.41 s | 2252 ms | 無政府状態に陥っていると表現したよ。I'DOHIKEISHIP。 |
| 67 | `fleurs_bench/ja/ja_086.wav` | 1028.58 s | 2504 ms | ここは南アフリカの主な観光地の一つであり南アフリカ国立公園。 |
| 71 | `fleurs_bench/ja/ja_091.wav` | 1091.84 s | 4680 ms | これらの理論ではある種の人々が日常的に特定のことをしたがる動機について。総理。 |

Every added text ends in a noun character before a closing mark, and in every one the utterance goes on after the candidate (the distance to the end is 2.2 s or more). The closing mark is written by the punctuation stage on a partial text (ASSUMPTION: not checked against the speaker's pause).

## Conditions and limits

- The rule is judged on the text only; the shortest-speech hold of `shadow_early_hold` is not applied (the live replay does not apply it either), so the figures compare the two completeness rules and nothing else.
- Each run is one pass over one recording; the counts are not a rate over repeated runs. The probe is deterministic on the same audio and models (the live runs reproduced the earlier figures exactly).
- The council's estimate of 3 noun-ending candidates inside lat-ja (utterances 5, 13 and 19, counted on partial texts) is confirmed for utterance 13 (index 12) only: the probe adds no candidate for utterances 5 and 19 (NOT VERIFIED why; the candidate texts of those two were not printed).
- `lat-en` is recorded to show that the shadow rule is the live rule outside Japanese. The held-out en sets are not part of this gate.
- The shared build outputs of `native/target/` were not used; the build is in `native/target-l2/` (outside git).
