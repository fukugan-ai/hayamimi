# hayamimi implementation status

[日本語](status.ja.md)

This English document is the canonical version; the Japanese version is a translation.
One row per slice gives its state, merge commit, where the verification comes from, how far its acceptance has passed, and the holes left.
What each slice builds and which acceptance it must pass is in section 8 of `rust_port.ja.md`; the requirements are in [requirements.md](requirements.md).
Commit hashes were checked with `git log --oneline`; main was at `f0aeaf4` when they were checked.

The three states:

- **Done**: merged, and a seat that did not write it has verified it.
- **In progress**: being built on a branch or in a worktree.
- **Not started**: not begun.

| Slice | State | Merge commit | Source of verification | Acceptance passed | Holes left |
|---|---|---|---|---|---|
| Conversation subtitles slice 1 (`hayamimi-relay`) | Done | `08cfdd4` | Section 8 of `rust_port.ja.md` ("done") | Steps 1–13 of the conversation-subtitles design | Holes in section 9 of the conversation-subtitles design |
| R0 (Python baseline and `hayamimi-eval`) | Done | `5bb6343`, `228a88b` | Subject line of `5bb6343` ("verified (RB-1..8)") | RB-1 to RB-8 | H14, H26 in `rust_port.ja.md` |
| R1 (measurement slice) | Done | `1b93167` | Body of `1b93167` (output of RC-1 to RC-4) | RP-17, RP-24 (verified in `83af5a5`); RP-7, RP-8, RP-20, RP-23 (passed with `--ignored` on Windows) | H29 (load split, re-measuring translation T), H30 (fp16), H31 (R7) |
| R1b (council and provisional values) | Done | D `68867d8`, T `92f2641`, tests `d921b21`, results `47341b4` | `68867d8`, which copies the council's verdict (kept outside git) into D | The 20/23 pair threshold and the 1% in evidence 3 are backed by measurements; `--min-silence` is set to 0.30 | H8 (2000 ms, 20% and 14,000 MiB kept as they are) |
| R2 (ja path, `--wav`, `/events`, CPU fallback) | Done | `f0aeaf4` | Independent verification passed on the second round (the verifier's reply, kept outside git); fixes in `a8840a7`; RP-6 acceptance re-set in `1d33a5b` and `255c6a0` | WSL run in the body of `a8840a7`: rp06 3, rp16 17, rp26 12, rp29 22, rp02 10, rp18 8, rp27 7 passed. T-RP-6-012 passed after RP-6 was re-set | H8c (RP-6 thresholds), H33 (match count of punctuation on the CPU fallback); T-RP-21-001, 005, 006, 007 belong to R10 |
| R3 (language routing) | Not started | none | none | RP-1, RP-22, the language-ID row of RP-16 and the R3 rows of RP-29 not run | H13, H24, H25 |
| R4 (speakers) | Not started | none | none | RP-15 and the R4 rows of RP-29 not run | none |
| R5 (capture and every server route) | Not started | none | none | RP-10, RP-11, RP-12 and the R5 rows of RP-29 not run | H34 |
| R6 (three Dioxus pages) | Not started | none | none | Page rows of RP-12 not run | The designer's boards come first |
| R7 (tightening recognition delay) | Not started | none | none | RP-4, RP-7 not run | H9 (0.25), H10, H31 |
| R8a–R8f (relay on the Rust `hayamimi`, translation speed) | Not started | none | none | RP-3, RP-5, RP-13, RP-25, RP-28 and conversation-subtitles CS-20 to CS-22, CS-25 not run | H6, H17, H32, H8b |
| R10 (fetching, documents, English versions) | Not started | none | none | RP-14, the spec row of RP-19, RP-21 not run | H16 |
| R11 (removing Python) | Not started | none | none | RP-9, RP-19 not run | The three conditions in 3.2 |

No slice is in progress at the moment.

## Not taken

| Not taken | Reason |
|---|---|
| Keeping progress only in `state.json` | It is outside git and does not reach the public repository, so a reader cannot see which slices are done |
| Adding a state column to the slice table in `rust_port.ja.md` | The design would change with every merge, and the place that defines "passed" would mix with the record of what passed |
| Pointing at line numbers | They move with every edit; slices and commits are named instead |
