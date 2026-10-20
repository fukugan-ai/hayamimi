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
- Since RP-83, `hayamimi-eval stream` writes `<run>.machine.jsonl` next to `--out`: one row (`t_us`, `load1`, `load5`, `load15`, `cargo`, `rustc`) before any audio, every 5 s (also while draining) and after the last event. It refuses to start (exit 2) when the first row has load1 >= 2.00, load15 >= 2.50 or a `cargo`/`rustc` process, and aborts (exit 3) when a build appears during the run.
- Each row also holds `io_some_total` and `io_full_total`, the `total=` fields (microseconds) of `/proc/pressure/io`, as evidence of a stalled disk. They are recorded and never judged. On a kernel without that file they are `null`, and `stream` writes one `stream.machine.pressure_unavailable` warning.
- The tests judge every Rust run with a machine file: first row load1 < 2.00 and load15 < 2.50, last row load5 < 2.50, no `cargo`/`rustc` in any row, rows at most 15 s apart. Python baselines are not judged.
- A Rust run without a machine file is accepted only if it is a record measured before the gate, listed by directory, name and sha256 of its jsonl (`rs1` to `rs3` of `rp32`, `e2e1`, `e2e2`). The test output names each run let through this way.

## Counted by the acceptance tests

Valid samples are those of 3.1 with `--min-silence` 300 ms.

| run | valid | translated | L p50 / p95 (ms) |
|---|---|---|---|
| e2e1 | 100 | 93 | 1415 / 1872 |
| py1 | 100 | 95 | 1478 / 2187 |
| e2e2 | 75 | 70 | 970 / 1303 |

`py1` has 101 samples with a `final`; one has `L_asr - L_dec` below 300 ms and is not valid.

## Records of later runs

New E2E runs are not written into `e2e1/` or `e2e2/`, because RP-66 reads those two directories as the records of main.
A run that belongs to a slice goes into `docs/results/<slice in lower case>/` (the windows of R8x go into `docs/results/r8x/`).
A file is named `<window or arm><N>.<e2e1|e2e2>.<kind>`, where kind is `.jsonl`, `.events.jsonl` or `.hayamimi.stdout.log`, and `.machine.jsonl` after R8x-1.
The load before and after the run goes into `<window or arm>.load.txt` in the form of `docs/results/rp32/load<N>.txt`.
A run made before the gate of RP-83 has the prefix `<window>pre` (for example `e1pre`). It is a record only and does not decide RP-67.
The other remains of a run stay outside git, in the work directory.

| file | content |
|---|---|
| `r8x/e1pre.e2e2.jsonl` | Rust run, `lat-en`, 15 utterances x 5 |
| `r8x/e1pre.e2e2.events.jsonl` | events received during that run |
| `r8x/e1pre.e2e2.hayamimi.stdout.log` | the log of `hayamimi` during that run |
| `r8x/e1pre.load.txt` | `/proc/loadavg` before and after that run |

`e1pre` is a record only and is not used for the verdict of RP-67.
The load average of the 15 minutes before the run was 3.50, and the load average of the 5 minutes after it was 3.88. Both are above the line of 2.50 that RP-83 sets, and there is no `machine.jsonl` for the run.
With `--min-silence` 120 ms, 74 of the 75 samples are valid and all 74 have a translation: `L` p50 666 ms, p95 1318 ms. Four samples are over 1200 ms. With 300 ms, the line of the acceptance test, 35 samples are valid: `L` p50 891 ms, p95 2879 ms.
The value of en for RP-67 comes from window E1, which is run again after R8x-1.

## 採らない手段 (not taken)

- The raw remains of a run (stdout, stderr, pid, gpu csv) are not copied here. They stay outside git, in the run's work directory.
- The tests do not read an environment variable or a list of paths outside git. The directories above are the fixed place.
- The Python baseline is one run (`py1`). The alternating pairs of RP-32 are judged by its own test and not by these two.
