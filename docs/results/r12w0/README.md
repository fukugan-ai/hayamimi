# Refine group cost before R12w1

One run of `hayamimi-eval der` over the five meetings of `testdata/eval_diar` measures how long the Rust route takes to re-diarize the groups, as seconds per group and as a ratio to the audio length. This is the cheap measurement (R12w0) that decides whether R12w1 can start. It is the time of re-diarizing the speakers; it does not include the time to re-decode the sentences, which R12w2 measures.

## Command

Built from commit `a74d6e8f490931aac87c21ba967ab85e7113ede1` (clean worktree), `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, CUDA build of the sherpa libraries, process priority nice -5.

    SHERPA_LIB=$HOME/.local/share/hayamimi/build/cuda1343/linux/install/lib
    cd native && CARGO_TARGET_DIR=<worktree>/target SHERPA_ONNX_LIB_DIR=$SHERPA_LIB \
      LD_LIBRARY_PATH=$SHERPA_LIB cargo build --release -p hayamimi-eval -j 2
    cd <repo> && LD_LIBRARY_PATH=$SHERPA_LIB \
      <worktree>/target/release/hayamimi-eval der --set testdata/eval_diar --collar 0.25

The build finished in 4 m 12 s. The `der` run took place on 2026-10-21 from 22:25:24 to 22:29:20 (+09:00), once, exit code 0. The only stderr line was one sherpa-onnx message that a circular buffer grew (`Overflow! ... Original data is copied. No data loss!`).

The `der` output, in full:

    [ES2011a] DER=17.5%  ref_speakers=4 hyp_speakers=5  ref_turns=83 hyp_segments=68  decode=35.2s  diar=26.9s over 32 groups (diar_rtf=0.045)  opened_by={'fast': 8}
    [IS1008a] DER=4.1%  ref_speakers=4 hyp_speakers=9  ref_turns=107 hyp_segments=88  decode=60.8s  diar=53.1s over 22 groups (diar_rtf=0.089)  opened_by={'fast': 6, 'remap': 3}
    [ES2004a] DER=17.3%  ref_speakers=4 hyp_speakers=8  ref_turns=131 hyp_segments=89  decode=32.3s  diar=25.0s over 39 groups (diar_rtf=0.042)  opened_by={'fast': 8, 'remap': 1}
    [IS1009a] DER=13.7%  ref_speakers=4 hyp_speakers=5  ref_turns=145 hyp_segments=123  decode=52.4s  diar=44.5s over 24 groups (diar_rtf=0.074)  opened_by={'fast': 6}
    [TS3003a] DER=13.9%  ref_speakers=4 hyp_speakers=6  ref_turns=62 hyp_segments=53  decode=50.5s  diar=41.6s over 28 groups (diar_rtf=0.069)  opened_by={'fast': 7, 'remap': 2}

    === mean DER over 5 meeting(s): 13.3% (collar=0.25s) ===

## Result

`refine_seconds` is the `diar=` value, `groups` the `over N groups` value, and `audio_seconds` is 600.0 for every meeting (`der_run.rs`, `Hypothesis`). The ratio is `refine_seconds / audio_seconds`. Seconds per group is `refine_seconds / groups`. The `diar=` values are printed to 0.1 s, so the ratios carry a rounding error of about 0.0001.

| meeting | refine_seconds | audio_seconds | ratio | groups | seconds per group |
|---|---|---|---|---|---|
| ES2011a | 26.9 | 600.0 | 0.045 | 32 | 0.84 |
| IS1008a | 53.1 | 600.0 | 0.089 | 22 | 2.41 |
| ES2004a | 25.0 | 600.0 | 0.042 | 39 | 0.64 |
| IS1009a | 44.5 | 600.0 | 0.074 | 24 | 1.85 |
| TS3003a | 41.6 | 600.0 | 0.069 | 28 | 1.49 |
| all five | 191.1 | 3000.0 | 0.0637 | 145 | 1.32 |

The last row is the ratio of the sums, not the mean of the ratios. A group here spans 20.7 s of audio on average (3000.0 s over 145 groups), close to the 25 s cap of `GROUP_MAX_SECONDS`.

Conditions around the run. The check made 9 s before the run read cargo 0, rustc 0, one-minute load 1.91. At the start of the run another seat's `cargo` was running (cargo 1, rustc 0, load 1.34). At the end cargo 1, rustc 1, load 4.73. A `cargo` of the merge seat of R13-5b had been running for 139 s at 22:29:32, so it started at about 22:27:13, inside this run. GPU memory used and utilization read 5603 MiB and 0 % both before and after the run; the process had already exited at the second sample, so this run's GPU use was not measured.

| point | time | cargo | rustc | load (1 min) | GPU memory used, utilization |
|---|---|---|---|---|---|
| before | 22:25:24 | 1 | 0 | 1.34 | 5603 MiB, 0 % |
| after | 22:29:20 | 1 | 1 | 4.73 | 5603 MiB, 0 % |

This run overlapped another seat's build, so the values include that load. The overall ratio is under 0.4, so no second run was made.

## Against the 0.5 line

The overall ratio 0.0637 is at or under 0.5, and so is every meeting (the largest is IS1008a at 0.089). Re-diarizing the groups takes about one sixteenth of the audio length, and the slowest meeting (IS1008a) averages 2.41 s per group. R12w1 can start.

This is the time of re-diarizing the speakers; it does not include the time to re-decode the sentences (R12w2 measures that with `Router::refine` on 25 s groups).
