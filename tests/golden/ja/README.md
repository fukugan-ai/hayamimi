# Japanese route golden set

Eight audio clips and the exact text hayamimi's Japanese route produces for
them. `docs/spec/ja_pipeline.md` (Japanese: `docs/spec/ja_pipeline.ja.md`)
describes that route in prose so another project can re-implement it; this
directory is the part a machine can check.

## Attribution

The clips come from **FLEURS** (`google/fleurs`, `ja_jp`), Copyright Google,
licensed **CC BY 4.0** (<https://creativecommons.org/licenses/by/4.0/>). They
are copied unmodified from `testdata/fleurs_bench/ja/`, which is the
benchmark set `docs/results/benchmarks.md` scores against; the reference text in
`golden.json` is that set's `manifest.json`, verbatim.

| file | FLEURS id | seconds |
|---|---:|---:|
| `ja_014.wav` | 1669 | 9.60 |
| `ja_024.wav` | 1674 | 8.70 |
| `ja_033.wav` | 1677 | 7.80 |
| `ja_036.wav` | 1679 | 6.48 |
| `ja_044.wav` | 1684 | 9.06 |
| `ja_079.wav` | 1702 | 9.90 |
| `ja_085.wav` | 1705 | 12.48 |
| `ja_090.wav` | 1710 | 11.22 |

## Why these eight

- **Lengths 6.5 s to 12.5 s.** FLEURS ja's shortest clip is 6.48 s, so there
  is no shorter end to sample; the top of the range is chosen to cross the
  12 s `max_speech` VAD limit's neighbourhood without becoming a long-form
  test.
- **`ja_085` and `ja_090` exercise the CJK ITN pass** (`hayamimi-asr::itn`),
  which is the stage most easily left out of a re-implementation. The
  recognizer emits `千九百四十年八月十五日` and `三十四件`; ITN rewrites them to
  `1940年八月15日` and `34件`. Note `八月` staying in kanji: a single bare
  numeral is never converted, which is the rule that keeps `一番`/`九州` intact.
- **`ja_044` records a real failure, on purpose.** Its first sentence
  (`アピアはサモアの首都です`) survives only as `あ。` — the head-dropout
  `docs/spec/ja_pipeline.md` describes, in the form the pre-roll cannot fix
  because the whole utterance, not just its head, is what the VAD cut short.
  Pinning it means a change that fixes it is visible as a change.
- **No clip's reference ends in `？`.** All 100 FLEURS ja references were
  checked and none contains a question mark at all — it is news and
  encyclopedia prose. So the question-mark rule in `hayamimi-asr::punct` is
  not covered here; the parity fixture on the `agent/feature/core-release`
  branch (`mobile/hayamimi_core/test/fixtures/punct_ja_parity.json`) covers
  it with synthetic cases instead.

## How the expected text was produced

`golden.json` is a record of a run of the Python engine made before R11.
That run built the same objects as the command line in the `cli` entry of
`_settings` in `golden.json`, and read the results off the event hub with a
listener, so what is recorded is the pipeline's own structured output, not
scraped stdout.
`finals` is one entry per `final` event (fast path, one per VAD segment, after
ITN and punctuation) and `refine` is the text of the last `refine` event (the
second pass over the whole utterance group).

There is no command in Rust that rebuilds `golden.json`.
The tool for it, `hayamimi-eval golden`, does not exist, and the design leaves
open whether it will (H104 of `docs/design/rust_port.ja.md`).
`golden_gpu.json` holds what the Rust route said for the same clips.

`golden.json` also records the CER of each recorded text against the FLEURS
reference. Those numbers are **transparency, not a threshold** — nothing
asserts them. They are there so a reader can see at a glance which clips the
pipeline handles well and which it does not.

## The two-level comparison rule

`native/hayamimi-asr/tests/rp02.rs` (RP-2) runs the eight clips through the
Rust route and passes when both of these hold (`check_golden`):

1. **All eight clips are present** in the result.
2. **Each clip's CER is no greater than its record.** The CER is scored
   against the FLEURS reference and compared as an integer in ten-thousandths
   with `cer_finals_vs_reference` in `golden.json`. The error names every
   clip that is worse.

The test also checks that each wav has the sha256 recorded in `golden.json`.
Run it with:

```
cd native && cargo test --release --jobs 2 -p hayamimi-asr --test rp02
```

The tests that run the models are marked `#[ignore]`, so the full run adds
`-- --ignored`.

The two-level rule described next is the rule of the Python test that existed
before R11 (exact match of `finals`, and CER within 1.0%, the `_cer_tolerance`
of `golden.json`). It explains the reason for a tolerance and does not run any
more.

The reason for the second level is that the recognizer runs int8 ONNX
kernels, and int8 kernels are not bit-reproducible across CPU
microarchitectures: onnxruntime dispatches to different vector code paths
depending on the instruction sets available, and the accumulated rounding can
render a character differently while transcribing the same words. Demanding
exact text on every machine would turn that into a failing test for a reason
that is not a regression.

### What 1.0% actually admits here, measured

Be honest about the size of that allowance. Normalized (NFKC, punctuation and
whitespace removed), the recorded texts are 21 to 44 characters long, so one
differing character is a CER of 2.3% to 4.8% on every clip in this set. **At
these lengths the 1.0% per-clip gate admits zero differing characters**, which
makes it, today, the same test as exact match.

It is kept anyway, for three reasons. It fails with a graded number instead of
a diff, so a report says how far off a run is. It stays meaningful if longer
clips are added later. And it states the intended contract, which is the thing
a re-implementation should copy: *pin the text, tolerate sub-percent drift*.

The test additionally prints a **set-level CER** over all eight clips
concatenated (291 normalized characters, so 1.0% there is about 2.9
characters). That figure is reported, never asserted.

If a different CPU turns out to produce benign single-character drift, the
threshold is what should be revisited — with the measurement written down —
not the golden text.

On the machine the file was recorded on, all eight clips match exactly, three
independent full runs in a row.
