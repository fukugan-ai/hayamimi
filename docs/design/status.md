# hayamimi implementation status

[日本語](status.ja.md)

This English document is the canonical version; the Japanese version is a translation.
One row per slice gives its state, merge commit, where the verification comes from, how far its acceptance has passed, and the holes left.
What each slice builds and which acceptance it must pass is in section 8 of `rust_port.ja.md`; the requirements are in [requirements.md](requirements.md).
Commit hashes were checked with `git log --oneline`; main was at `f0aeaf4` when they were checked.

The five states:

- **Done**: merged, and a seat that did not write it has verified it.
- **Implemented, awaiting verification**: committed on a branch, not merged, and no seat that did not write it has verified it yet.
- **In progress**: being built on a branch or in a worktree.
- **Planned**: not begun, but placed in a wave of the 07:00 plan ("07:00 の計画" in section 8 of `rust_port.ja.md`). W1 runs 14:00–20:00 JST on 2026-10-14, W2 21:00–01:00, W3 01:45–04:15 and W4 04:45–05:45 on 2026-10-15.
- **Not started**: not begun and not placed in a wave.

| Slice | State | Merge commit | Source of verification | Acceptance passed | Holes left |
|---|---|---|---|---|---|
| Conversation subtitles slice 1 (`hayamimi-relay`) | Done | `08cfdd4` | Section 8 of `rust_port.ja.md` ("done") | Steps 1–13 of the conversation-subtitles design | Holes in section 9 of the conversation-subtitles design |
| R0 (Python baseline and `hayamimi-eval`) | Done | `5bb6343`, `228a88b` | Subject line of `5bb6343` ("verified (RB-1..8)") | RB-1 to RB-8 | H14, H26 in `rust_port.ja.md` |
| R1 (measurement slice) | Done | `1b93167` | Body of `1b93167` (output of RC-1 to RC-4) | RP-17, RP-24 (verified in `83af5a5`); RP-7, RP-8, RP-20, RP-23 (passed with `--ignored` on Windows) | H29 (load split, re-measuring translation T), H30 (fp16), H31 (R7) |
| R1b (council and provisional values) | Done | D `68867d8`, T `92f2641`, tests `d921b21`, results `47341b4` | `68867d8`, which copies the council's verdict (kept outside git) into D | The 20/23 pair threshold and the 1% in evidence 3 are backed by measurements; `--min-silence` is set to 0.30 | H8 (2000 ms, 20% and 14,000 MiB kept as they are) |
| R2 (ja path, `--wav`, `/events`, CPU fallback) | Done | `f0aeaf4` | Independent verification passed on the second round (the verifier's reply, kept outside git); fixes in `a8840a7`; RP-6 acceptance re-set in `1d33a5b` and `255c6a0` | WSL run in the body of `a8840a7`: rp06 3, rp16 17, rp26 12, rp29 22, rp02 10, rp18 8, rp27 7 passed. T-RP-6-012 passed after RP-6 was re-set | H8c (RP-6 thresholds), H33 (match count of punctuation on the CPU fallback); T-RP-21-001, 005, 006, 007 belong to R10 |
| R3a (language routing: `route` and `lid` in `hayamimi-asr`) | Done | `7d82842` | Independent verification passed (the verifier's reply, kept outside git) | Passed: the language-ID rows of RP-16 (T-RP-16-018 to 026) and the R3 rows of RP-29 as fixtures (T-RP-29-022 to 026); RP-1 waits for R3b. The 79 RP-19 names of `route.rs` and `lid.rs` pass as unit tests against a scripted recognizer and identifier. Language-ID rows of RP-16: fixtures T-RP-16-018 to 022 in `rp16.rs`; real-model runs T-RP-16-023 to 026 in `hayamimi-asr/tests/rp16_lid.rs` (CPU, GPU, model missing); T-RP-16-027 to 030 in `hayamimi-asr/tests/gpu_guard.rs` (device hidden with `Gpu::Try`: the identifier and the punctuation model go to the CPU and the process lives; the same run without the device count ends it); T-RP-16-031 to 034 in `lid_dual.rs` (the 8 golden clips: whisper-tiny alone names 6 as ja, the language left after the dual confirmation is ja for all 8). R3 rows of RP-29 as fixtures only (T-RP-29-022 to 026). RP-1 not run (it needs the R3b `scorecard`); no real run of the R3 rows of RP-29, because the `hayamimi` binary does not route yet | The `Decode` trait is faked until R3b merges; language ID is not yet wired into the `hayamimi` binary (`Engine::open` does not call `lid::open`; owner R3b, T-RP-16-035); opening the CUDA and ORT sessions concurrently in one process can abort (2 of 9 parallel runs); R3b puts a process-wide lock on session creation in `hayamimi-gpu` (T-RP-16-036, 037); `--lang auto` stays unavailable until the binary is wired; Korean word spacing and the user's dictionary come after the router |
| R3b (recognizers, residency, `scorecard`, `lat-multi`, language-ID wiring, session lock) | In progress | none | none | RP-1, RP-22 and the real runs of the R3 rows of RP-29 not run | H13, H24, H25; SenseVoice's language tag (`Decoded.tag`) is not in the safe result of sherpa-onnx 1.13.8 (`offline_asr.rs` `get_result` parses only text, tokens and timestamps; the stream pointer is `pub(crate)`), so the `Decode` implementation must read the library's JSON `lang` through `sherpa-onnx-sys` |
| R0b (test support finds the main tree's `testdata/` from a worktree) | Done | `32ecb2a` | Independent verification passed (the verifier's reply, kept outside git) | `hayamimi-eval` tests FAILED 0 in a worktree with no `testdata/`. Negative control: with both search places pointed at missing paths, the test fails naming both paths. Symlinks: 0 | Not run: a clone with no `testdata/` in either the main tree or the worktree (the wording in section 8 of `rust_port.ja.md`); the verifier checked the same failure by replacing the second search place instead |
| R4 (speakers) | Planned (W1) | none | none | RP-15 and the R4 rows of RP-29 not run | none |
| R5 (capture, partials and every server route) | Planned (W1) | none | none | RP-10, RP-11, RP-12 and the R5 rows of RP-29 not run | H34 |
| R6 (three Dioxus pages, `/ui` wiring) | Done | `7446f75` | The independent verification seat, the screen-test seat and the designer seat (their replies, kept outside git) | Verification: `cargo test` FAILED 0 with 419 passed; the W7 negative control failed 3 tests as intended; clippy 0; conflicts 0; leaks 0. Screen test: BD-1 to BD-11 and W7 passed; header 60.80 px. BD-12 passed (designer seat: 6 differences, all accepted, 0 to fix) | NOT VERIFIED: the DOM with the fallback band shown (band height and `role="status"`); checked in the E2E screen check |
| R7 (tightening recognition delay) | Planned (W2) | none | none | RP-4, RP-7 not run | H9 (0.25), H10, H31 |
| R8a–R8g (relay on the Rust `hayamimi`, translation speed, outside sinks) | R8a: verified (independent seat: CS-21/CS-22 pass with negative controls; 10-minute real-audio run belongs to E2E). R8b: verified. R8c: verified. R8d: planned in W2; R8e, R8f, R8g (Zoom, Teams, SRT): planned in W3 | R8a: `c868eb0`; R8b: `c663ce6`; R8c: `9278273`, chat-endpoint fix `022b283` | R8a: independent verification passed (the verifier's reply, kept outside git). R8b: independent verification passed, re-run on main (the verifier's reply, kept outside git). R8c: independent verification passed, with two integration-level negative controls; the chat-endpoint fix passed a separate independent verification (the verifier's replies, kept outside git) | R8a: conversation-subtitles CS-21 and CS-22 passed with a fake recognizer and negative controls. R8b: CS-20 rows 001 to 004, the translation rows of RP-16 (T-RP-16-038 to 050), the GPU-check negative control and the default port 8080 passed; `cargo test --release --no-fail-fast` on main FAILED 0. R8c: CS-5, CS-25, RP-25 rows 001 to 009, the RP-29 sink scene (fake destination) and T-CX-1-002 and 006 passed; RP-25 row 010 waits for the RP-3 run (R8e). R8c fix: against the server on 8080, the relay and curl differ on 0 of 40 probe1 lines, while the old `/completion` form differs on 40 of 40; 38 of 40 match the probe1 log; the request-body test and its negative control pass; no test yet checks `timings.cache_n`. Not run: RP-3, RP-5, RP-13, RP-28 | H6, H17, H32, H8b. R8b not run: T-RP-16-051 (on a machine with a GPU) and CS-20 row 005 (the E2E wave) |
| R10 (fetching, documents, English versions) | Planned (W3) | none | none | The spec row of RP-19 and RP-21 not run (RP-14 moved to R8f) | H16 |
| R11 (removing Python) | Planned (W4) | none | none | RP-9, RP-19 not run | The three conditions in 3.2 |
| End-to-end acceptance (E2E-1 mic ja→en, E2E-2 loopback en→ja) | Planned (after W4, 05:45–06:45) | none | none | E2E-1, E2E-2 not run | A Japanese utterance spoken into the real microphone cannot be checked by machine |

No slice is in progress at the moment.

## Not taken

| Not taken | Reason |
|---|---|
| Keeping progress only in `state.json` | It is outside git and does not reach the public repository, so a reader cannot see which slices are done |
| Adding a state column to the slice table in `rust_port.ja.md` | The design would change with every merge, and the place that defines "passed" would mix with the record of what passed |
| Pointing at line numbers | They move with every edit; slices and commits are named instead |
