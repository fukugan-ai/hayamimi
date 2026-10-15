# E2E-1 and E2E-2 records

The acceptance tests `e2e1_mic_process_ja_to_en_beats_python` and `e2e2_loopback_process_en_to_ja_within_budget` (`native/hayamimi-eval/tests/e2e1.rs`, `e2e2.rs`) read the files in this directory.
Only the samples and events jsonl of a run are kept here.

| file | content |
|---|---|
| `e2e1/e2e1.jsonl` | Rust run, `lat-ja`, 23 utterances x 5, port 8766 |
| `e2e1/py1.jsonl` | Python baseline for E2E-1 |
| `e2e2/e2e2.jsonl` | Rust run, `lat-en`, 15 utterances x 5, loopback sink |
| `e2e2/e2e2.events.jsonl` | events received during the E2E-2 run |

## Conditions of the run

- The Rust `hayamimi` and relay are the build of main `7ea558f`. The Rust processes ran at nice 0.
- `py1` ran with the Windows Python, with refine left enabled.
- `llama-server` ran at nice -5 from 11:26.

## Counted by the acceptance tests

Valid samples are those of 3.1 with `--min-silence` 300 ms.

| run | valid | translated | L p50 / p95 (ms) |
|---|---|---|---|
| e2e1 | 100 | 93 | 1415 / 1872 |
| py1 | 100 | 95 | 1478 / 2187 |
| e2e2 | 75 | 70 | 970 / 1303 |

`py1` has 101 samples with a `final`; one has `L_asr - L_dec` below 300 ms and is not valid.

## 採らない手段 (not taken)

- The raw remains of a run (stdout, stderr, pid, gpu csv) are not copied here. They stay outside git, in the run's work directory.
- The tests do not read an environment variable or a list of paths outside git. The directories above are the fixed place.
- The Python baseline is one run (`py1`). The alternating pairs of RP-32 are judged by its own test and not by these two.
