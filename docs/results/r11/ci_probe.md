# R11c: the CI in a clone without `testdata/` and `models/`

Measured 2026-10-20. The clone is made with `git clone --no-local`, holds neither `testdata/` nor `models/`
(`ls -d testdata models` exits 2), and builds in its own `native/target`. The tree is this branch merged with
the branches of R12a1 (`rp19`) and R12a2 (`rp09`). The result of the run is in the last section.

## Test binaries the CI leaves out

Each binary below fails in the clone because it reads `testdata/` or `models/`, which `git` does not carry.
`rp80` checks that every binary the test step leaves out has a row here with such a reason, and that no other binary is left out.

| binary | reason | crate |
|---|---|---|
| `r6` | reads `models/silero_vad.onnx` | hayamimi |
| `rp12` | reads `testdata/baseline/wire/events.jsonl` | hayamimi |
| `decode` | reads `testdata/eval_real` | hayamimi-asr |
| `preload` | reads `models/silero_vad.onnx` | hayamimi-asr |
| `rp18` | reads `models/silero_vad.onnx` | hayamimi-asr |
| `rp68` | reads `models/silero_vad.onnx` | hayamimi-asr |
| `r0b` | reads `testdata/` | hayamimi-eval |
| `rb01` | reads `testdata/` | hayamimi-eval |
| `rb02` | reads `testdata/` | hayamimi-eval |
| `rb03` | reads `testdata/` | hayamimi-eval |
| `rb04` | reads `testdata/` | hayamimi-eval |
| `rb05` | reads `testdata/` | hayamimi-eval |
| `rb06` | reads `testdata/` | hayamimi-eval |
| `rb07` | reads `testdata/` | hayamimi-eval |
| `rb08` | reads `testdata/` | hayamimi-eval |
| `rp01` | reads `testdata/baseline` | hayamimi-eval |
| `rp04` | reads `testdata/` | hayamimi-eval |
| `rp05` | reads `testdata/` | hayamimi-eval |
| `rp13` | reads `testdata/` | hayamimi-eval |
| `rp14` | reads `testdata/` | hayamimi-eval |
| `rp15` | reads `testdata/baseline` | hayamimi-eval |
| `rp32` | reads `testdata/` | hayamimi-eval |
| `rp33` | reads `testdata/` | hayamimi-eval |
| `rp12_pages` | reads `testdata/baseline/wire` | hayamimi-ui |
| `rp17` | reads the ReazonSpeech model under `models/`, and needs the GPU build of sherpa-onnx | hayamimi-gpu |

## Tests the CI skips by name

Four unit tests of `hayamimi-asr` open `libonnxruntime.so.1` by the soname of the GPU build of sherpa-onnx.
The CPU archive that the build script downloads ships `libonnxruntime.so` only, so on Linux they fail with `ort: failed to load from libonnxruntime.so.1`.
They are skipped by name: `a_missing_model_directory_leaves_every_stage_out_and_the_engine_silent`,
`an_engine_with_no_route_has_no_early_holds`, `an_engine_with_no_route_has_no_recognizers_to_report`,
`with_no_stage_on_the_gpu_there_is_nothing_to_give_back`.
Three tests of `hayamimi` (`rp16`) hide the GPU and expect the reason `no_device`.
That reason needs a driver library that loads and reports no device.
A GitHub runner has no `libcuda.so.1`, so the stage reports `driver_unreadable` and the three tests fail with `reason of `gpu.device.unavailable` is not no_device`.
They are skipped by name: `missing_cuda_build_falls_through_to_the_cpu_build`, `hidden_gpu_translate_server_starts_on_cpu_with_a_reason`,
`rp16_no_gpu_translate_server_falls_back_to_cpu_and_says_so`.
The first run on Windows is not measured here.

## Programs the test step needs first

`rp27` runs `rg` (ripgrep) to count imports, and the runner does not carry it, so the workflow installs it (`apt-get` on Linux, `choco` on Windows).
`r8g_media` starts the stand-in programs `fake_recognizer` and `fake_ffmpeg` from `target/release/examples/`.
`cargo test --examples` builds them as test executables with a hash in the name, not at that path, so the workflow builds them with `cargo build --release --workspace --examples` before the test step.
A local target that had once run `cargo build --all-targets` already held them, which is why the clone run passed.

## Result of the `run:` lines

Measured 2026-10-20 on Linux (WSL2, rustc 1.99.1), one pass of the `run:` lines in the order of `test.yml`, by `rp80` case 018 (`the_run_lines_pass_in_a_clone_without_testdata_or_models`).
The step that installs ALSA headers is skipped (it needs the administrator).
Windows is not measured here.

| step | result |
|---|---|
| fetch the sherpa-onnx libraries | `cargo build --release -p sherpa-onnx-sys`, rc 0; `SHERPA_ONNX_LIB_DIR` points at `target/sherpa-onnx-prebuilt/<archive>/lib` |
| `cargo fmt --all --check` | rc 0 |
| `cargo clippy --workspace --all-targets --jobs 2 -- -D warnings` | rc 0 |
| `cargo test … --test <90 names> -- --skip …` | rc 0; 109 `test result:` lines, 2692 passed, 0 failed, 101 ignored |
| RP-19 rows not yet ported | rc 101 (allowed): `172 rows of the table are not in the list yet` |
| RP-81 owned exceptions | rc 101 (allowed): `owned exceptions remain, per owner: R13-10: 50, R13-14: 18` |

The tree is this branch merged with the branches of R12a1 and R12a2, because `rp80` and `rp19` need the files of both.
`rp17` does not compile without `SHERPA_ONNX_LIB_DIR` (`env!` in `native/hayamimi-gpu/tests/rp17.rs`), and clippy, the rp19 listing and the test step compile it, so the fetch step sets the variable once for the later steps.
Case 019 (`an_excluded_binary_really_fails_in_a_clone_without_testdata_or_models`) passes in the same clone.
