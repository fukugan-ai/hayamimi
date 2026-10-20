# VAD window probabilities after the end of an utterance

The probe `native/hayamimi-asr/tests/vad_trace_probe.rs` (`#[ignore]`, an offline probe) runs `models/silero_vad.onnx` through `ort` and prints the speech probability of every 512-sample window of `testdata/lat-ja.wav` and `testdata/lat-en.wav`. No decision code (`vad.rs`, `pipeline.rs`, `endpoint.rs`) is changed.

    cd native && cargo test -j 2 --release -p hayamimi-asr --test vad_trace_probe -- --ignored --nocapture

## Model

The file (sha256 `9e2449e1...`, 643,854 bytes) has inputs `x [1, 512]`, `h [2, 1, 64]`, `c [2, 1, 64]` and outputs `prob [1, 1]`, `new_h`, `new_c`. It takes one window per call with no context and no sample-rate input, and carries the LSTM state `h`, `c` across windows (zero at the start). sherpa-onnx runs this layout on its v4 path.

## Method

- `orig`: the file as it is.
- `noise60`: runs of exact zeros of at least 512 samples inside the gap after an utterance are replaced by white noise at -60 dBFS RMS (measured -60.00 dBFS; 528,000 samples in ja, 336,000 in en).
- `zero_only`: 20 s of zeros from a fresh state. The maximum probability is 0.0429 over 625 windows, so zeros alone never reach 0.35 or 0.5.
- A fall is the first window that starts at or after the manifest end (and before the next utterance) whose probability is under the cutoff, as milliseconds after the manifest end.

## Result

Milliseconds after the manifest end, per utterance. `gap` is the length until the next utterance. In ja utt 10, 11, 13, 14, 20, 21 and 22 the gap starts with 1500 ms of exact zeros; in the other utterances the audio continues for a while after the manifest end and the zeros come later.

| lang | utt | gap ms | orig < 0.5 | orig < 0.35 | noise60 < 0.5 | noise60 < 0.35 |
|---|---|---|---|---|---|---|
| ja | 1 | 3500 | 48 | 48 | 48 | 48 |
| ja | 2 | 4020 | 192 | 288 | 128 | 256 |
| ja | 3 | 3900 | 596 | 756 | 84 | 756 |
| ja | 4 | 3920 | 348 | 380 | 412 | 508 |
| ja | 5 | 3820 | 472 | 632 | 472 | 632 |
| ja | 6 | 4020 | 56 | 248 | 56 | 248 |
| ja | 7 | 4360 | 720 | 848 | 688 | 848 |
| ja | 8 | 4160 | 56 | 56 | 56 | 152 |
| ja | 9 | 1750 | 236 | 492 | 300 | 428 |
| ja | 10 | 1500 | 31 | 31 | 31 | 31 |
| ja | 11 | 1500 | 126 | 318 | 126 | 254 |
| ja | 12 | 1732 | 222 | 510 | 30 | 30 |
| ja | 13 | 1500 | 18 | 274 | 18 | 242 |
| ja | 14 | 1500 | 183 | 343 | 119 | 311 |
| ja | 15 | 3861 | 571 | 795 | 507 | 763 |
| ja | 16 | 2616 | 398 | 654 | 302 | 590 |
| ja | 17 | 1969 | 665 | 825 | 633 | 857 |
| ja | 18 | 1895 | 24 | 24 | 24 | 24 |
| ja | 19 | 1511 | 17 | 17 | 17 | 17 |
| ja | 20 | 1500 | 312 | 472 | 248 | 536 |
| ja | 21 | 1500 | 421 | 613 | 389 | 805 |
| ja | 22 | 1500 | 315 | 539 | 411 | 763 |
| ja | 23 | 915 | 147 | 403 | 179 | 403 |
| en | 1 | 1970 | 24 | 24 | 24 | 24 |
| en | 2 | 1905 | 18 | 18 | 18 | 18 |
| en | 3 | 2015 | 1 | 1 | 33 | 33 |
| en | 4 | 2080 | 6 | 38 | 38 | 38 |
| en | 5 | 1910 | 54 | 54 | 54 | 54 |
| en | 6 | 1755 | 32 | 32 | 32 | 64 |
| en | 7 | 1960 | 181 | 181 | 181 | 213 |
| en | 8 | 1835 | 101 | 101 | 197 | 421 |
| en | 9 | 1795 | 6 | 6 | 6 | 358 |
| en | 10 | 1745 | 7 | 7 | 199 | 423 |
| en | 11 | 2230 | 198 | 198 | 198 | 390 |
| en | 12 | 1670 | 20 | 20 | 244 | 308 |
| en | 13 | 1795 | 26 | 26 | 90 | 346 |
| en | 14 | 1705 | 11 | 11 | 75 | 331 |
| en | 15 | 354 | 2 | 2 | 34 | 66 |

## Findings

- In exact zeros the probability falls as a slope, not a step. After ja utt 21 (zeros from the manifest end) it reads 0.98, 0.94, 0.87, 0.81, 0.76, 0.72, 0.69 ... 0.50 at 421 ms and 0.35 at 613 ms, in 32 ms steps. The LSTM state carries the speech forward; zeros alone stay under 0.05 from a fresh state.
- Replacing the zeros with -60 dBFS noise does not remove the slope: ja utt 21 reaches 0.35 at 805 ms instead of 613 ms. In en the noise moves the 0.35 position later in 11 of 15 utterances and earlier in none; in ja the 0.35 position moves earlier in 8 of 23 utterances, later in 6 and stays in 9, so the direction is not fixed there.
- The segment end that sherpa-onnx reports equals the start of the first window under 0.35 after the manifest end plus 52 ms in 32 of 37 utterances with a segment end (`end_diff` of `vad-tail.log` at cutoff 0.5). The 52 ms is `Tail - min_silence` of `voice-activity-detector.cc` (the 300 ms silence is subtracted again), that is the 32 ms window plus 20 ms (ten 512-sample windows are 320 samples longer than the 4800-sample silence). The sum "0.35 position + 300 ms + 20 ms" does not match: it is 268 ms too late in all 32.
- The five that differ: ja utt 2 (+340 ms, the probability rises again before the end), ja utt 18 and 19 (the segment ends before the manifest end, so the end belongs to another stretch), en utt 9 (+20 ms) and en utt 12 (+180 ms).
