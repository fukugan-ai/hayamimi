# E2E-1 and E2E-2 records

The acceptance tests `e2e1_mic_process_ja_to_en_beats_python` and `e2e2_loopback_process_en_to_ja_within_budget` (`native/hayamimi-eval/tests/e2e1.rs`, `e2e2.rs`) read the files in this directory.
Of a run, the samples jsonl, the events jsonl and the `hayamimi` stdout log (`<run>.hayamimi.stdout.log`, where the tests read the rule that closed each sample) are kept here. The four records listed under "Counted by the acceptance tests" were measured before that log existed and have none.

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
- `stream` takes `--own-pid <pid>` once for each of the run's three processes: `hayamimi`, the relay and the translation `llama-server`. `--own-llama-port <port>` goes with it, names the port that `llama-server` listens on, and has no default (the window's server listens on 18084); each of the two options needs the other, and `stream` stops with a usage error (exit 2, no machine record) when one is missing. The pids come from the listening ports, for example `ss -ltnpH 'sport = :8766'` for `hayamimi` (its `--ws-port`), the same for the relay's port, and `ss -ltnpH 'sport = :<port>'` for the server. The pid of `stream` itself is not given. A process started through `sudo -n nice` has the pid of `sudo` as `$!`; give the pid of the process that listens.
- With `--own-pid`, `stream` checks the pids before it keeps the first row, in the order given, and refuses the run (exit 2, `stream.machine.refused` with `reason` `own_pid`, `own_fault` and `own_pid`) at the first pid that fails: `unreadable` (`comm` or `stat` cannot be read), `comm` (not `hayamimi`, `hayamimi-relay` or `llama-server`), `port` (a `llama-server` whose `/proc/<pid>/cmdline` does not hold the word `--port` once, followed by the number `--own-llama-port` names written in decimal; the one-word form `--port=<port>` is refused). After every pid has passed, a set that does not hold each of the three names exactly once is refused as `set`. `own_pid` is asked before `load1`, `load15` and a build. Without `--own-pid` the record has none of the fields below.
- With `--own-pid`, the first row also holds `own` (the pids and names) and `clk_tck` (100), and every row holds `own_cpu_ticks` (the sum of `utime` and `stime` of the three `/proc/<pid>/stat` files, in clock ticks, `null` when one cannot be read any more) and `cpu_some_total` (the `total=` of the `some` line of `/proc/pressure/cpu`, recorded and never judged). Stop the three processes only after `stream` has ended and written its last row; a process that is gone first makes the last row's `own_cpu_ticks` `null` and the run fails.
- The tests judge every Rust run with a machine file: first row load1 < 2.00 and load15 < 2.50, last row load5 < 2.50, no `cargo`/`rustc` in any row, rows at most 15 s apart. Python baselines are not judged. When the file has `own`, the last row's load5 is judged less the run's own share: the rate of `own_cpu_ticks` between two rows, in cores, smoothed over 300 s from 0 in the first row and cut down to a whole hundredth (cut, not rounded). The test output has one line for each such run, for example `e3q1.e2e2 load5 3.10 own5 0.611407 (61) judged 249 limit 250`, and `own absent` in place of `own5` for a file without `own`.
- A Rust run without a machine file is accepted only if it is a record measured before the gate, listed by directory, name and sha256 of its jsonl (`rs1` to `rs3` of `rp32`, `e2e1`, `e2e2`). The test output names each run let through this way.

## Counted by the acceptance tests

A sample is valid when `L_asr - L_dec` reaches the line of the rule that closed it, as in RP-32. A Rust sample closed by the `early` rule has a line of 120 ms (`EARLY_MIN_SILENCE_MS`); every other Rust sample, and every Python sample, has `--min-silence` (300 ms). The rule comes from `<run>.hayamimi.stdout.log` (`asr.endpoint.decided`), joined to the `final` of the events file by `t_us` and the number in its `id`. The tests refuse a `final` with no `decided` line, an `id` that has two, and two `final` at one `t_us` with different rules.
The runs below were measured by builds without the early rule and have no stdout log. They are listed by directory, name and sha256 (`e2e1` and `e2e2` of this directory, `rs1` and `e2e2c` of `docs/results/r8h_verify/`) and are counted with every sample at 300 ms; the test output says so. A Rust run that is not on that list, or is listed under another sha256, is refused for the missing log.

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
With the line of each sample (120 ms for the 57 samples closed by `early`, 39 of which fall under 300 ms; 300 ms for the 17 closed by `silence`), 74 of the 75 samples are valid and all 74 have a translation: `L` p50 666 ms, p95 1318 ms. Four samples are over 1200 ms. With every sample at 300 ms, 35 samples are valid: `L` p50 891 ms, p95 2879 ms. The acceptance tests print both counts for a run that has a stdout log, and `the_l9_record_counts_74_samples_by_the_per_sample_line_and_35_by_300_ms` pins them.
The value of en for RP-67 comes from window E1, which is run again after R8x-1.

## 採らない手段 (not taken)

- The raw remains of a run (stderr, pid, gpu csv) are not copied here. The `hayamimi` stdout log is kept, because the per-sample line needs it. They stay outside git, in the run's work directory.
- The tests do not read an environment variable or a list of paths outside git. The directories above are the fixed place.
- The Python baseline is one run (`py1`). The alternating pairs of RP-32 are judged by its own test and not by these two.
