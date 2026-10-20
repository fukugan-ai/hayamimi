# Contributing to hayamimi

## Setup

Build the executable and fetch the models. The build needs the GPU build of
sherpa-onnx first (see [`docs/guide/build_gpu.ja.md`](docs/guide/build_gpu.ja.md),
Japanese only).

```bash
cd native
SHERPA_ONNX_LIB_DIR=<sherpa-onnx install/lib> cargo build --release -p hayamimi
hayamimi models fetch --dir models
```

## Tests

Run the tests of one crate at a time, with the project's two build jobs:

```bash
cd native
cargo test --release --jobs 2 -p hayamimi-asr
```

The unit-level logic (language routing, fallback behavior, etc.) is tested in
the `#[cfg(test)]` modules next to the code in `native/hayamimi-asr`, and
the other crates carry theirs the same way.

## Accuracy evaluation

Model or routing changes must be validated against real speech, not just
unit tests -- this project's whole design history (`docs/results/benchmarks.md`) is
built on real-speech regressions that unit tests alone would have missed
(e.g. iteration #27's Cantonese regression from a routing change that passed
every existing test).

- `hayamimi-eval scorecard` -- scores the engine on the real-speech sets and
  writes the scorecard as JSON (the kind of table `docs/results/scorecard.md`
  holds).
- `hayamimi-eval stages` -- times every stage of the Rust port on one
  provider and writes a run file (RTF and the time of each stage).
- `hayamimi-eval chrf` and `hayamimi-eval der` -- score translations with chrF
  and speaker labels with DER.

Run them as `cargo run --release -p hayamimi-eval -- <command> --help` from
`native/`.

**Policy: any change to model routing, decoding parameters, or
language-detection logic should be re-validated by re-running the relevant
eval script against real speech clips before merging, not just against
synthetic/TTS test data.** TTS audio is too clean to catch the failure modes
that matter here (see `docs/eval/eval.md` vs. `docs/eval/eval_real.md` for why this
project moved from synthetic to real-speech evaluation early on). If you
touch language-detection or routing logic specifically, re-run the eval
across *all* languages, not just the one you changed -- several regressions
in this project's history were caught only because of a full re-score
(see `docs/results/benchmarks.md` iterations #12 and #27).

## Documentation

`docs/` is grouped by what a reader wants from it. The index is
[`docs/README.md`](docs/README.md) (Japanese: `docs/README.ja.md`) and every
new document belongs in it.

```
docs/README.md, docs/README.ja.md   the index
docs/guide/    how to embed and how to tune it
docs/spec/     the Japanese route, specified for re-implementation
docs/results/  current numbers -- kept up to date
docs/eval/     experiment records -- dated, never rewritten afterwards
docs/design/   design investigations behind a decision
docs/verify/   procedures for checking something on real hardware
docs/research/ the 2026-08-23 pre-code research snapshot
docs/images/   images the READMEs use
```

Where a new document goes: a number you intend to keep current goes in
`results/`; the run that produced it goes in `eval/` with its date in the
title. An investigation you did to reach a decision goes in `design/`, even
if the decision was "no". A procedure someone will follow goes in `verify/`.
If it tells a user how to use or configure hayamimi, it is `guide/`, not
`design/`.

**Language.** English is primary and Japanese sits alongside it, in the
`README.md` + `README.ja.md` pattern: an English file `x.md` has its Japanese
counterpart at `x.ja.md`. Documents written in Japanese before this policy
stay Japanese; the index tags each file's language so nobody has to open one
to find out.

**How to write it.** The house style, used everywhere in this repo:

- Problem first, then the change, then the reasoning, then the effect. Not a
  bare list of what changed -- a reader should be able to tell why it was
  worth doing.
- Explain a term once, the first time it appears (VAD, refine, LID, RTF).
- A number only ever appears with the conditions that produced it: which
  audio, which model, which machine, how many samples.
- Negative results are recorded too. "Measured, not adopted" with the numbers
  is worth as much as a win, and this project's history is full of them.

Run `cargo run --release --manifest-path native/Cargo.toml -p hayamimi-eval -- doc-links`
from the repository root after moving or renaming anything under `docs/`.
It checks that every Markdown link points at a file that exists.

## Pull requests

- Keep unrelated changes out of one PR.
- If you change decode parameters, routing thresholds, or add/replace a
  model, include the before/after eval numbers in the PR description.
- If you add a new model dependency, update `THIRD_PARTY_NOTICES.md` and
  the model list of `hayamimi models fetch` (`native/hayamimi/src/models_fetch/items.rs`)
  in the same PR.
