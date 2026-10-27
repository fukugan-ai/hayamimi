# Realtime Translator

## About this repository

The **original**, [oboroge0/hayamimi](https://github.com/oboroge0/hayamimi), is real-time multilingual speech recognition that runs on the CPU alone, with no GPU and no cloud API (its runtime is Python).
This repository ([fukugan-ai/hayamimi](https://github.com/fukugan-ai/hayamimi)) is a **fork** of it: a copy that keeps the original history and is developed separately.
Nothing from this fork is pushed or sent as a pull request to the original.
The badges below this section point to the original's CI and releases, not to results of this fork.
The text after this section describes the original Python version.
Statements such as "no GPU" remain in it and do not apply to the Rust version.

### What is being done here

The Python runtime is being replaced with Rust (the Cargo workspace in `native/`), and inference uses the GPU (CUDA).
The work is complete when Python is out of the runtime path and removed.
Speed comes first; quality (recognition error rate and naturalness of translation) comes second.
The speed measure is **L**: the delay from the end of an utterance to the appearance of its translated subtitle.
L is measured at p50 (the median) and p95 (the delay within which 95% of utterances fall).
Once the speed target is met, quality must match or beat the Python version.

<!-- Rewrite this section to the current state on each merge; do not append dated sections or progress paragraphs. Progress lives in docs/design/status.ja.md. -->
### Current state
- The 78 Python files are deleted, and the version before the removal can be taken from the tag `archive/python-before-r11` (RP-9 row of the [status table](docs/design/status.md)).
- All of the Python tests have been ported to Rust tests (RP-19 row of the [status table](docs/design/status.md)).
- End-to-end checks on played-back recorded audio pass for Japanese to English and for English to Japanese (E2E; build of 2026-10-15; [records](docs/results/e2e/)).
- A check with a person speaking Japanese into a real microphone is not done yet (NOT VERIFIED; microphone row of the [status table](docs/design/status.md)).
- The final judgement of the delay has not been run yet (window E4; RP-67 row of the [status table](docs/design/status.md)).
- The delay cannot be called passed until a check that the way the end of an utterance is placed does not sway the result is also done (RP-73; the implementation and tests are in, but the acceptance test fails until the window E4 record exists; RP-73 row of the [status table](docs/design/status.md)).
- Recognition runs on the CPU (int8) (R7 row of the [status table](docs/design/status.md)).
- A measurement puts the CPU ahead of the GPU, but the machine load during that measurement is high, so the value is unverified (NOT VERIFIED).
- The translation server used in the E2E windows loads all layers on the GPU (`-ngl 99`; [unit file](native/hayamimi-relay/units/hayamimi-llama-e2e.service)).
- In the production command, the refine pass (re-reading each group of utterances) runs by default, and `--no-refine` stops it. When the refine pass cannot run, the screen and the log say so, and the run continues without it.
- Vulnerabilities in dependencies are checked every morning against the official advisories (RustSec and others) and the lock files. On 2026-10-27 three TLS crates were raised: rustls to 0.23.46, rustls-webpki to 0.103.16 and webpki-roots to 1.0.11. webpki-roots 1.0.10 takes the emSign ECC Root CA - G3 out of the trusted list, because the private keys of the intermediates under it became public. The critical advisory on zip (RUSTSEC-2026-0367 to 0369) is not taken in: sherpa-onnx-sys uses zip only when its build unpacks an iOS archive, which the Linux and Windows builds never do (see "Not taken" in [the status table](docs/design/status.md)).
- The two-way face-to-face mode (Japanese and English at the same time on one microphone) is in up to the design and test rows (acceptance IP-1 to IP-9) and four of its steps. The first is the tool that builds interleaved trial recordings, the two-way translation test, and naming the translation server port with an argument (acceptance IP-1, IP-6, IP-8). The second is `--mode pair`, which falls back to Japanese or English instead of stopping when language identification misses (acceptance IP-3; tested with a fake language identifier and a fake speech recognizer, not yet verified on real recordings). The third is the page that also reads the relay translation stream (`?relay=`) and shows both directions on one page. The fourth is judging IP-5 line by line, and writing one log line when a segment ends without becoming a sentence (acceptance IP-9). IP-5 passed in a run that starts all four processes ([record](docs/results/in-person/ip05.md); the translations in that run are the fixed sentences a fake translation server returned). Together with `--mode pair`, a conversation that alternates between Japanese and English can be translated in both directions from one microphone and shown on one page. The runs of IP-2 (an alternating recording is played through without gaps) and IP-7 have also passed ([IP-2 record](docs/results/in-person/ip02.md), [IP-7 record](docs/results/in-person/ip07.md)). The utterances read in the wrong language are recorded, and they were within the provisional line. The record of IP-4 (the latency difference between `--mode pair` and `--lang`) has also been taken. The median of the difference is within the line, the 95th percentile is outside it, and some clips could not be joined ([record](docs/results/in-person/ip04.md)). The pair records that the tests with hand-counted values read are kept under names no later run rewrites (`ip04_pair.*`, `br04_ip_s8.*`), and CI counts that no test file other than the owner of a run names the records that run rewrites. With this, acceptance IP-1 to IP-9 have all been run on real objects ([design](docs/design/conversation_subtitles.ja.md), section 10; Japanese only). The designs and acceptance test tables for the speed and quality work, the three-pane page with a light and dark switch, and choosing the language pair are in (two of the speed measures are implemented too: deciding the language of a draft in the middle of an utterance, and not queuing the settled translation behind draft translations; the rest of the implementation has not started). The design for a page that sends the microphone and Teams audio from a browser on another PC is in too (not implemented yet; it needs a check on the sending PC first). Even when the page is open to the LAN, the settings and reset routes answer only the same PC. The tools that record the delay from the end of an utterance to the subtitle, stage by stage, are in too (the subtitle content is unchanged). Their tests run in CI; the runs that need a model, recordings or a browser stay out of it and run on a machine that has them. One of these runs recorded the values before the speed work ([record](docs/results/in-person/br03_baseline.md)). The 30-utterance recording that alternates Japanese and English was played three times. Over the 76 utterances that became subtitles (the 2 right after start-up are left out), the delay from the end of an utterance to its translated subtitle being drawn on the page had a median of 1002 ms and a 95th percentile of 1688 ms. The targets are 750 ms and 1300 ms, and both are exceeded. The translation came from a server on the GPU of the same machine, only one run was measured, and the values are not guaranteed on another machine. After that, the two measures above (the flags `--partial-lid` and `--partial-yield`; each can be switched off to get the earlier behaviour) went in, and runs with the flags off and on were measured one after another on the same build ([record](docs/results/in-person/br03_ip_s78.md)). Over the same 76 utterances, the delay until the translation is drawn on the page went from a median of 1032 ms to between 950 and 968 ms, and from a 95th percentile of 1755 ms to between 1593 and 1615 ms. The targets of 750 ms and 1300 ms are still exceeded. For English utterances the wait at the end of speech grew by between 17 and 43 ms at the median, because the English draft is read by a heavier recognizer (hole H71 in section 10.13.10 of the design, which is in Japanese only). All four of these runs overlapped the nightly NAS copy, and the values are not guaranteed on another machine.
- The delays in the table below are values from a build made before the refine pass was connected, and the values with the refine pass are measured in window E4 (RP-61 row of the [status table](docs/design/status.md)).

| Language | p95 | 1600 ms line | 1200 ms target |
|---|---|---|---|
| Japanese (run 1) | 1484.8 ms | inside | not reached |
| Japanese (run 2) | 1474.1 ms | inside | not reached |
| English | 1089.1 ms | inside | reached |

The table shows the p95 of `L` from the latest real-machine runs (RP-67 row of the [status table](docs/design/status.md)).

### Related documents
- State of each slice: [docs/design/status.md](docs/design/status.md)
- Design (Japanese only): [docs/design/rust_port.ja.md](docs/design/rust_port.ja.md)
- Build and run guide (Japanese only): [docs/guide/run_hayamimi.ja.md](docs/guide/run_hayamimi.ja.md)

[![tests](https://github.com/oboroge0/hayamimi/actions/workflows/test.yml/badge.svg)](https://github.com/oboroge0/hayamimi/actions/workflows/test.yml) [![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![release](https://img.shields.io/github/v/release/oboroge0/hayamimi)](https://github.com/oboroge0/hayamimi/releases)

**Real-time, multilingual speech-to-text on CPU only.** Live subtitles, a
browser dashboard, speaker labels, and on-the-fly translation -- no GPU, no
cloud API, under 2GB RAM.

日本語版 README は [README.ja.md](README.ja.md) にあります。

The command, crate and repository names stay `hayamimi` (早耳), Japanese for
"quick ear" -- someone who picks up on things fast. That's the design goal: partial subtitles appear while you're
still talking, and a finalized line lands roughly **100ms after you stop**.

## Why

Most CPU-only real-time transcription setups fall back to a single
general-purpose model (Whisper) and accept its accuracy ceiling. Realtime
Translator instead routes each utterance to whichever specialist model is best for its
language, all running as quantized (INT8) ONNX models via
[sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) -- no PyTorch, no CUDA.

On real broadcast Japanese audio (see `docs/results/scorecard.md`), that routing
gets **3.8% CER** (remeasured 2026-09-01 with the head-dropout fix and CJK
number normalization in the pipeline), vs `whisper-large-v3-turbo`'s 13.8%
on the same clips, while running at 10-50x realtime on a 6-core desktop CPU.
A full comparison against Whisper variants, cloud STT APIs, and other local
models -- including the languages where Realtime Translator loses -- is in
`docs/results/comparison.md`.

## Features

| Feature | What it does |
|---|---|
| 5-route language catalog | ja/zh/ko/yue/en+24 EU languages each go to a dedicated best-in-class model; everything else (~1600 languages) falls back to Meta's Omnilingual ASR |
| Partial subtitles | in-progress draft text updates every ~0.5s while you're still speaking |
| Fast finals | a finalized line typically lands ~100ms after you stop talking (ja; see `docs/design/goals.md` for other languages) |
| Two-pass refinement | after 2s of silence, recent utterances are batch re-decoded for a higher-accuracy "clean" transcript (ja real-broadcast CER 15.5% -> 12.0%) |
| Speaker labels | `--speakers` tags each utterance S1/S2/... live via CAM++ nearest-centroid matching, then the refine pass re-diarizes each group with pyannote segmentation-3.0 and remaps clusters onto the same S{n} labels (mean DER 25.7% -> 13.9% on 5 AMI meetings, see Limitations) |
| Translation | `--translate en,zh,ko,es,...` translates Japanese lines live (en via FuguMT; any other M2M-100 target code is accepted if the model's vocabulary supports it -- zh/ko/es have measured quality, see docs/design/translate_m2m.md) |
| Hotwords / user dictionary | `--hotwords` biases decoding toward proper nouns (currently has no effect on the ja tier -- see Limitations); `--replace` does post-hoc find/replace and works everywhere |
| CJK number normalization | conservative kanji-numeral -> arabic-digit conversion (ja/zh/yue only): magnitude numbers, 点-decimals, and digit-string years/codes; idioms/proper nouns are left alone by default (the Rust version is `hayamimi-asr::itn`) |
| Runtime dictionary APIs | with `--serve`, `GET`/`POST /replacements` and `/itn_overrides` swap the find/replace and ITN exclude/force dictionaries mid-session |
| Structured events + runtime config | every stage of the pipeline (finals, translations, model loads, warnings, session summaries...) publishes to an `EventHub` an embedding app can listen to directly; `--serve` additionally exposes `GET`/`POST /config` and `POST /reset` for changing language/translation/VAD settings and resetting a session without restarting the process -- see "Embedding: runtime control and structured events" below |
| OBS overlay + dashboard | `--serve` starts a local HTTP server with a browser-source overlay and a live dashboard |
| Network audio input | `--input ws` accepts mic audio over a WebSocket (phone, ESP32/stackchan) and feeds it through the same pipeline, including `--serve`'s dashboard/overlay |
| Speaker loopback input | `--input speaker` transcribes whatever is playing on the PC's audio output (WASAPI loopback, Windows only, no Stereo Mix setup needed); `--input mix` sums it with the mic into one stream (e.g. meeting transcription) |
| English tier v2 (opt-in) | `--en-tier v2` swaps the default multilingual Parakeet v3 for an en-only Parakeet TDT v2 (needs `hayamimi models fetch --en-parakeet-v2`, +~660MB resident); measured FLEURS en WER 10.0% -> 6.6% (`docs/eval/en_candidates.md`) |
| 4-class ja punctuation (opt-in) | `--punct-model 4class` swaps the default BERT restorer for a single model predicting 、/。/？/！ directly (adds real ？/！ support the default model lacks); int8 ~37MB, ~4.6ms/line on FLEURS ja, but trained on dense web text -- underperforms the default on sparse-punctuation domains like TV captions (`docs/eval/punct_retrain.md`, see Limitations). Needs `hayamimi models fetch --punct-4class` |
| Memory-bounded | LRU model eviction keeps resident models under a configurable cap (default: <2GB total) |
| CPU-only | every model runs as quantized ONNX via sherpa-onnx; no GPU or PyTorch required |

## Demo UI

`--serve` starts a local server exposing three views:

- **`http://localhost:8833/dashboard`** -- the live dashboard: a partial-text
  strip for in-progress speech, a finals feed with language badges, speaker
  chips, and per-line latency, inline translations under each line, and a
  second column with the refined (two-pass) transcript as it lands.
- **`http://localhost:8833/`** -- a minimal OBS browser-source overlay
  (add this URL as a Browser Source in OBS for stream captions). The
  confirmed line and the in-progress line are separate rows; append
  `?show=final` or `?show=partial` to render only one of them, so each
  can be placed and styled as its own OBS source.
- **`http://localhost:8833/transcript`** -- plain scrolling transcript
  history.

![dashboard](docs/images/dashboard.png)

🎬 **[Watch the demo video](https://github.com/oboroge0/hayamimi/releases/download/v0.1.0/hayamimi_demo.mp4)** — real 4-language audio (ja/en/ko/zh) transcribed live, replayed frame-accurately from a captured session.

## Network audio input

`--input ws` runs a WebSocket ingest endpoint instead of reading the local
microphone, so a phone or a stackchan-class ESP32 board can stream mic audio
over the LAN and get it transcribed through Realtime Translator's normal pipeline:

```bash
hayamimi --input ws --serve
# -> ws://<host>:8766/ingest accepts audio; http://localhost:8833/dashboard shows the results
```

Protocol: connect to `/ingest`, send one JSON text frame
(`{"sr": 16000, "format": "pcm_s16le", "channels": 1}`), then stream raw
`pcm_s16le` audio as binary frames. The server resamples non-16kHz audio and
replies with the same partial/final/translation/refine JSON events the
dashboard's SSE stream carries, so a client can show its own subtitles too.
Only one audio-producing client is accepted at a time. The reference client
that streams a wav file to `/ingest` at real-time pace is `hayamimi-eval stream`;
the way it sends audio is also a model for a phone/ESP32 implementation.

`--input ws` binds `127.0.0.1` by default, so the endpoint is localhost-only
until you opt in with `--ws-host 0.0.0.0` -- do that only on a network you
trust, since `/ingest` has no authentication. The bound address is printed
to stderr on startup either way.

## Speaker (system audio) loopback input

`--input speaker` transcribes whatever is playing through the PC's audio
output -- the other side of a call, a video -- instead of the microphone;
`--input mix` transcribes the mic and the speaker output summed into one
stream, e.g. for meeting transcription (your voice + the call audio
together):

```bash
hayamimi --input speaker
hayamimi --input mix
```

This uses WASAPI loopback (through the Rust `wasapi` crate), which taps the
render endpoint directly -- unlike the classic "Stereo Mix" recording
device, no opt-in is needed in Windows Sound settings, and it works even
when the sound driver doesn't expose Stereo Mix at all. **Windows only**
(no loopback path is wired up for other platforms yet).

`mix` matches capture timestamps within roughly 48ms and clips the summed
samples to [-1, 1]. It uses the microphone as its clock and drops old queued
audio on both sides if decoding falls behind. A failed speaker capture is
reported on stderr and falls back to mic-only; a failed microphone stops the
stream. This is one mixed transcript, with no separate tracks or acoustic
echo cancellation. Use headphones to avoid capturing the call audio twice.

By default both modes use the system's default input/output device. If you
have more than one mic or output device, `--mic-device NAME` /
`--speaker-device NAME` (substring match) pick a specific one.
Over a Remote Desktop session, the PC's own
physical mic usually can't be opened at all (Windows blocks cross-session
access) -- enable microphone redirection in your RDP client instead (in
`mstsc`: Show Options > Local Resources > Remote audio > Recording >
"Record from this computer"), or run hayamimi at the local console.

## Embedding in another app

The Rust version's parts live in `native/hayamimi-asr`: language routing is
`hayamimi-asr::route`, speech detection is `hayamimi-asr::vad`, and the
Japanese recognition flow is `hayamimi-asr::pipeline`. `hayamimi` is the
executable that calls them. With `hayamimi --serve`, the running session gains
`GET`/`POST /config`, `POST /reset`, and `GET`/`POST /replacements` and
`/itn_overrides`, and the structured JSON events arrive on `GET /events`.
The full guide -- the event table, the `/config` keys, and what `POST /reset`
does -- is in [`docs/guide/embedding.md`](docs/guide/embedding.md).
For Flutter/Dart, see [`mobile/hayamimi_core/README.md`](mobile/hayamimi_core/README.md).

## Requirements

The Rust version of `hayamimi` is built from the Cargo workspace in `native/`.
It needs a GPU build of sherpa-onnx first (steps are in
[`docs/guide/build_gpu.ja.md`](docs/guide/build_gpu.ja.md) and
[`docs/guide/run_hayamimi.ja.md`](docs/guide/run_hayamimi.ja.md), both Japanese only).
Developed and tested on **Windows 11**.
Audio capture covers the microphone on Windows and Linux, and speaker loopback on Windows only.
On a machine with no capture path, such as macOS, use `--input ws` (`/ingest`) or a wav file.
A real-device run of the Linux microphone has not been checked here (NOT VERIFIED: run `hayamimi --input mic` on a Linux machine and watch the subtitles).

The translation server for languages outside Japanese and English (port 18081) is started on llama.cpp v0.7.0 (build 11669) with `-np 1 -c 768 -ngl 99 -cram 0` ([`hayamimi-llama-other.service`](native/hayamimi-relay/units/hayamimi-llama-other.service)). The trial translation server for Japanese and English (port 18083) is started with `-ngl 99 -c 4096 -np 4 -cram 0` ([`hayamimi-llama-pair.service`](native/hayamimi-relay/units/hayamimi-llama-pair.service)). GPU memory rose from its level before the start by 4263 MiB on 18081 and 3281 MiB on 18083 (one measurement on 2026-10-22, taken just after `/health` answered). That is 238 MiB and 678 MiB less than with the previous argument (`-c 8192`), and the 43 and 41 translated sentences that were measured came out the same as before. `-fa on` and KV cache quantization are not used: on 18083 they changed the translation and made it slower.

## Quickstart

Build the executable, fetch the models, then run.
Type the steps one at a time.

```bash
# 1. Build the executable (SHERPA_ONNX_LIB_DIR is the install/lib of the GPU sherpa-onnx build)
cd native
SHERPA_ONNX_LIB_DIR=<sherpa-onnx install/lib> cargo build --release -p hayamimi

# 2. Fetch the models into models/ (git-ignored)
hayamimi models fetch --dir models

# 3. Real-time transcription from your microphone
hayamimi --input mic

# From the PC's audio output instead (a call, a video) -- Windows only
hayamimi --input speaker

# Mic + PC audio output together, e.g. for meeting transcription
hayamimi --input mix

# With the dashboard + OBS overlay
hayamimi --input mic --serve
# -> open http://localhost:8833/dashboard in a browser
```

With the default target directory, the executable is `native/target/release/hayamimi`.
`models fetch` fetches every group by default.
`--minimal` fetches only the four groups of the ja/en core: the ReazonSpeech int8 recognizer, the whisper-tiny language identifier, the Silero VAD and the BERT punctuation model.
Two opt-in groups are added with their own flags: `--en-parakeet-v2` adds the en-only Parakeet v2, and `--punct-4class` adds the 4-class Japanese punctuation model; either combines with `--minimal`.
The download size has not been measured for the Rust version (NOT VERIFIED: run `du -sh models` after the fetch).
See `THIRD_PARTY_NOTICES.md` for what each model's license commits you to.

## CLI reference

The table below lists the flags and defaults of the original Python runtime.
The flags the Rust `hayamimi` accepts are shown by `hayamimi --help`, and their use is in [`docs/guide/run_hayamimi.ja.md`](docs/guide/run_hayamimi.ja.md) (Japanese only).

| Flag | Default | Description |
|---|---|---|
| `--wav PATH` | mic input | simulate streaming from a 16kHz mono WAV file instead of the microphone |
| `--no-realtime` | off | with `--wav`, don't sleep between chunks (fast batch processing) |
| `--input {mic,wav,ws,speaker,mix}` | mic, or wav if `--wav` is given | audio source; `ws` accepts audio over the network (see below); `speaker` transcribes the PC's audio output (WASAPI loopback, Windows only); `mix` sums mic + speaker into one stream |
| `--mic-device NAME` | system default input | substring match for the input device `--input mic`/`mix` captures from; see `--list-audio-devices` |
| `--speaker-device NAME` | system default output | substring match for the output device `--input speaker`/`mix` loops back from; see `--list-audio-devices` |
| `--list-audio-devices` | off | print available audio devices and exit |
| `--ws-host HOST` | `127.0.0.1` | bind host for `--input ws`'s `/ingest` endpoint; pass `0.0.0.0` to accept LAN clients |
| `--ws-port PORT` | 8766 | port for `--input ws`'s `/ingest` endpoint |
| `--threads N` | 4 | inference threads per model |
| `--no-partial` | off | disable in-progress draft subtitles |
| `--min-silence SEC` | 0.35 | silence duration that ends an utterance; lower = snappier finals, more splits |
| `--max-speech SEC` | 12.0 | force-finalize an utterance after this many seconds of continuous speech |
| `--max-resident N` | 3 | max non-tier0 models kept resident (LRU eviction); `<=0` = unlimited |
| `--serve [PORT]` | off, 8833 | serve the dashboard + OBS overlay at `http://localhost:PORT` |
| `--no-refine` | off | disable the second-pass re-decode of utterance groups |
| `--transcript PATH` | none | append refined transcript lines to this file |
| `--hotwords PATH` | none | hotword list (one per line) to bias decoding toward proper nouns -- **currently has no effect on the ja tier** (ReazonSpeech's byte-level BPE tokens.txt can't encode them; a startup warning tells you how many failed). Use `--replace` for ja proper nouns instead |
| `--replace PATH` | none | user dictionary: `wrong=right` per line, applied to all output |
| `--mode {single,balanced,fast,pair}` | `balanced` | language-switching preset. `balanced` switches only when both language detectors agree (see `docs/eval/lid.md`); `single` pins the language given by `--lang` and never switches; `fast` switches on every detection, matching pre-v0.2.0 behavior. `pair` is for face-to-face use and falls back to Japanese or English when identification misses (tested with a fake identifier; not yet verified on real recordings). Individual flags below override the preset |
| `--lang-switch-guard SEC` | 2.0 | treat a new-language detection shorter than this as noise: it can never count toward confirming a switch (see `--lid-switch-confirm`) and it suppresses the omnilingual fallback on an empty decode (`0` disables) |
| `--lid-switch-confirm N` | 2 | consecutive new-language detections (each >= `--lang-switch-guard` long) required before the session actually switches language; raise for stickier single-language sessions |
| `--speakers` | off | label utterances with speaker ids (S1, S2, ...); the refine pass re-diarizes each group with pyannote segmentation-3.0 |
| `--speaker-remap-threshold T` | 0.35 | cosine-similarity threshold for mapping the refine pass's local diarization clusters onto the session's global S{n} labels (the live fast path keeps its own 0.45 threshold) |
| `--translate [LANGS]` | off, `en` | translate Japanese lines to these comma-separated languages. `en` uses the dedicated FuguMT module; any other M2M-100 target code (`zh`, `ko`, `es`, `fr`, ...) is accepted if the model's vocabulary supports it -- unvalidated targets (anything outside `zh`/`ko`/`es`) print a quality-not-measured note, see docs/design/translate_m2m.md |
| `--en-tier {v3,v2}` | `v3` | which Parakeet model handles `en`; `v3` also covers the other 24 V3_LANGS European languages. `v2` is opt-in, en-only, lower WER (needs `hayamimi models fetch --en-parakeet-v2`; degrades to `v3` with a `model_fallback` event if not downloaded) -- see docs/eval/en_candidates.md |
| `--punct-model {bert,4class}` | `bert` | which model restores ja punctuation. `bert` is the currently shipped Mojicast BERT-char restorer (comma/period from the model + a `？` suffix heuristic, no `！`); `4class` is opt-in, predicts 、/。/？/！ directly (needs `hayamimi models fetch --punct-4class`; degrades to `bert` with a `warning` event if not downloaded) -- see docs/eval/punct_retrain.md |

## Architecture

```
                          ┌─────────────┐
  mic / wav ───────────▶ │  Silero VAD │  0.35s end-of-speech + 0.8s preroll
                          └──────┬──────┘
                                 │ speech segment
                                 ▼
                   ┌───────────────────────────┐
                   │  whisper-tiny spoken-LID   │  runs on first ~4s while
                   │  (+ char-set arbitration)  │  the segment is still coming in
                   └─────────────┬─────────────┘
                                 │ language tag
                 ┌───────────────┼────────────────┬─────────────┬──────────────┐
                 ▼               ▼                ▼             ▼              ▼
             ┌───────┐      ┌─────────┐      ┌──────────┐  ┌─────────┐   ┌──────────┐
             │  ja   │      │   zh    │      │  ko/yue  │  │ en + 24 │   │  ~1600   │
             │ Reazon│      │Paraformer│      │SenseVoice│  │EU langs │   │  other   │
             │Speech │      │   -zh   │      │  small   │  │Parakeet │   │Omnilingual│
             │Zipform│      │         │      │          │  │TDT v3   │   │  ASR     │
             └───┬───┘      └────┬────┘      └────┬─────┘  └────┬────┘   └────┬─────┘
                 └───────────────┴────────────────┴─────────────┴─────────────┘
                                                │
                     partial (every ~0.5s)      │      final (~0.1s after end-of-speech)
                     ◀───────────────────────────┴───────────────────────▶
                                                │
                          ┌─────────────────────┼─────────────────────┐
                          ▼                     ▼                     ▼
                 ┌────────────────┐   ┌──────────────────┐   ┌────────────────┐
                 │ ja punctuation  │   │ speaker labeling  │   │  translation    │
                 │ (BERT restore)  │   │ (CAM++, --speakers)│   │ (FuguMT/M2M-100)│
                 └────────────────┘   └──────────────────┘   └────────────────┘
                                                │
                     2s silence: batch re-decode recent utterances (two-pass refine)
                                                │
                                                ▼
                              dashboard / OBS overlay / transcript file
```

Models are lazy-loaded on first use; an LRU cache evicts the
least-recently-used non-Japanese models (`--max-resident`) so memory stays
bounded no matter how many languages a session wanders through.

## Measured performance

The values in this section are from the measurement dated in the heading of each subsection; the current values are in "Current state" at the top.

### End-to-end delay of the Rust version (measured 2026-10-15)

**L** is the delay from the moment the last audio sample of an utterance is sent to the moment the translated subtitle of that utterance appears.
The p50 and p95 are computed over valid samples that got a translation only.
Samples without a translation do not enter L, so the share that got one is judged separately.

| Measurement | Direction | Valid samples | Samples with a translation | L p50 | L p95 |
|---|---|---|---|---|---|
| E2E-1 Rust | ja→en | 100 | 93 | 1415 ms | 1872 ms |
| E2E-1 Python (the comparison) | ja→en | 100 | 95 | 1478 ms | 2187 ms |
| E2E-2 Rust | en→ja | 75 | 70 | 970 ms | 1303 ms |

Both checks passed. The conditions and results:

- E2E-1: the share with a translation is at least 20/23 (about 0.87), and the Rust L p50 and p95 are both no greater than Python's.
  The results were 93/100, p50 1415 ms against 1478 ms, and p95 1872 ms against 2187 ms.
- E2E-2: the share with a translation is at least 20/23, and the L p95 is at most 2000 ms.
  The results were 70/75 and a p95 of 1303 ms (the p50 is reported and not judged).

Conditions of the measurement:

- Build: main at 7ea558f. The Rust processes ran at nice 0, and llama-server ran at nice -5 from 11:26.
- Python baseline: a single run (py1) on the Windows Python with refine left enabled.
- Samples: E2E-1 played 23 recorded Japanese utterances (`lat-ja`) five times each, 115 samples. E2E-2 played 15 recorded English utterances (`lat-en`) five times each, 75 samples.
- Valid sample: the delay from the end of the utterance to the final text, minus the time from the end-of-speech decision to the final text, is at least the `--min-silence` value of 300 ms.
- Valid counts: in E2E-1, 100 of the 115 samples for both Rust and Python; in E2E-2, all 75.

What this measurement does not show:

- The Python baseline is one run, and the verdict of the alternating pairs under the same load that the design asks for (RP-32) is not part of this record.
- The machine load before each run is not part of this record.
- It played recorded audio, so a person speaking Japanese into a real microphone has not been checked (NOT VERIFIED: check by speaking on a real machine and watching the subtitle and its translation).
- The record does not say whether the E2E-2 capture came from the output device of a real Windows machine (NOT VERIFIED: check by running E2E-2 again on a real Windows machine).

The per-run sample jsonl files and the conditions are in `docs/results/e2e/` (commit `9103c95`).
A later E2E-2 record, `e1pre` (`docs/results/r8x/`), is a record only and does not decide RP-67, because the machine load around the run was above the RP-83 line of 2.50.
### Alternating Rust and Python measurement (RP-32, 2026-10-20)

Python and Rust were run in 3 alternating pairs under the same load; this is the formal verdict of E2E-1.
The values come from the `--ignored` output of `rp32` and from the E2E seat's report; this document did not recount them.
The record of this run is outside the repository, and `docs/results/rp32/` holds the run before the change of E's form.

| Item | Result | Condition |
|---|---|---|
| Per-row line | 18 of 19 utterances are no slower than Python | 17 are required |
| 300 ms line | 17/19 | the second line in the `rp32` output |
| L p50 and p95 | 885 ms, 1501 ms | the run before the change of E's form had 865 ms and 1461 ms |
| Share of utterances with two or more `final` | Rust 22.61 %, Python 8.70 % | smaller is better |
| CER | Rust 13.15 %, Python 12.95 % | smaller is better |

What this measurement does not show:

- On quality, Rust is still worse than Python on both the split `final` share and CER.
- The cause of the L p95 getting worse than in the run before the change of E's form is not identified.
- The Python baseline is fixed to the 3 pairs of 2026-10-15 and was not re-run.

### Speaker diarization and recognition on YouTube meetings (measured 2026-10-22)

Six YouTube meeting videos under the Creative Commons Attribution license were used as audio that has no reference speaker turns.
The measurement reports two things.
One is how many speakers the speaker diarization (the step that splits speech by who spoke) found, against the number named in each video's description.
The other is the character error rate (CER) of Japanese recognition, against the uploader's captions, which are the uploader's own text and not a verified reference.
**RTF** (real-time factor) is the processing time divided by the audio length.

Speaker count:

| Video id | Language | Seconds used | Named in the description | Found | Difference | Diarization seconds | RTF |
|---|---|---|---|---|---|---|---|
| JPfo6wXM0BY | en | 1200 (first 20 min) | 4 | 5 | +1 | 101.7 | 0.085 |
| nfchIbbkaxE | en | 1548.9 | 2 | 13 | +11 | 115.1 | 0.074 |
| yZ78p5dvnRU | en | 820.1 | 2 | 2 | 0 | 79.6 | 0.097 |
| Laef392EX6g | ja | 2369.1 | 4 | 9 | +5 | 226.9 | 0.096 |
| HfoqGsKAALg | ja | 2384.4 | 2 | 6 | +4 | 237.3 | 0.100 |
| rVRnAq0K318 | ja | 2408.2 | 3 | 7 | +4 | 219.8 | 0.091 |

The found count equals the named count in 1 of 6 videos (17%) and is within ±1 in 2 of 6 (33%).
The count bias is +4.2, so the diarization over-counts speakers; for one video with 2 named speakers it found 13.
The diarization ran on the CPU (`embedder.rs:61`, `vad.rs:85`); the GPU was not used for it.

Japanese recognition against the uploader's captions:

| Video id | Clips | CER | RTF |
|---|---|---|---|
| Laef392EX6g | 27 | 35.9% | 0.045 |
| HfoqGsKAALg | 27 | 31.9% | 0.028 |
| rVRnAq0K318 | 26 | 28.3% | 0.034 |
| all | 80 | 32.2% | 0.036 |

Conditions of the measurement:

- Clips: the first 10 minutes of each Japanese video, cut at caption boundaries into clips of about 20 seconds.
- Tool: the `scorecard` command with `Gpu::Try`.
- Build: main at 38712d0.
- Audio: converted to 16 kHz mono.

What this measurement does not show:

- The DER (diarization error rate) cannot be computed on these videos, because they have no reference speaker turns. The speaker count is compared with the names in the description, which is not a reference of the speaker turns.
- For reference, the AMI meeting ES2011a, which has reference speaker turns, gives a DER of 17.5% (4 speakers in the reference, 5 found, diarization RTF 0.045). This is one meeting and is separate from the 5-meeting AMI mean (13.9%) in the speaker-label row of the feature table.
- The CER is measured against the uploader's captions, which are not a verified reference.
- The device used for recognition is not confirmed (NOT VERIFIED: `Gpu::Try` tries the GPU first, and only the diarization is confirmed to run on the CPU).
- The English WER (word error rate) could not be measured, because the three English videos have no uploader captions.

Sources and license:

- <https://www.youtube.com/watch?v=JPfo6wXM0BY>
- <https://www.youtube.com/watch?v=nfchIbbkaxE>
- <https://www.youtube.com/watch?v=yZ78p5dvnRU>
- <https://www.youtube.com/watch?v=Laef392EX6g>
- <https://www.youtube.com/watch?v=HfoqGsKAALg>
- <https://www.youtube.com/watch?v=rVRnAq0K318>

All six videos are under the Creative Commons Attribution license (the license YouTube labels as allowing reuse); the license version is not given, because YouTube does not show one.
The uploader of each video is shown on the linked page.
The audio was used on the local machine only for this analysis, deleted after the measurement, and not distributed.
The results come with no warranty.

### Clip-level scoring of the original Python version (re-measured 2026-09-01)

End-to-end (LID -> routing -> decode -> ja punctuation), real speech, no
preroll/two-pass (single clips). `en` uses WER, all others use CER (`yue`
t2s-normalized). Full methodology in `docs/results/scorecard.md`.
**RTF** (real-time factor) is the processing time divided by the audio length.

| Language | Clips | LID accuracy | Route | Mean error | Mean RTF |
|---|---|---|---|---|---|
| ja | 15 | 15/15 | ReazonSpeech | 3.8% | 0.090 |
| en | 15 | 15/15 | Parakeet v3 | 2.3% | 0.102 |
| zh | 12 | 12/12 | Paraformer-zh | 6.6%* | 0.084 |
| ko | 12 | 12/12 | SenseVoice | 8.1% | 0.060 |
| yue | 12 | 12/12 | SenseVoice | 6.1% | 0.043 |

An RTF of 0.102 or lower on every route means each route
runs about 10-23x faster than realtime on CPU alone -- see `docs/design/goals.md` for the
full target table and `docs/results/benchmarks.md` for the complete iteration log
(30+ measured changes, latency/memory/accuracy tradeoffs and why each one was
made or rejected).

Headline numbers from that log:

- **Japanese CER 3.8%** on real broadcast audio, vs. 13.8% for
  `whisper-large-v3-turbo` on the same clips. The optional refine-time ja
  second opinion (`--refine-ja-second-opinion`) measured 4.0% on a separate
  50-minute broadcast set (hayamimi-paper harness).
- *zh's 6.6% includes ~1.3pt of numeral-notation mismatch: the pipeline now
  writes arabic numerals ("1000") where some references spell them in kanji
  ("一千") -- a scoring-convention gap, not misrecognition.
- **~100ms mean final latency** (ja, punctuated); ~236ms mean / 552ms max
  across a 5-language soak test with every feature enabled.
- **<2GB RAM** with `--max-resident 3` (1.35GB at `--max-resident 2`).

## Limitations (honest list)

- **Code-switching mid-sentence is not supported.** The router picks one
  language per utterance; a sentence that mixes Japanese and English within
  itself will have the minority-language portion mangled or dropped.
  Utterance-level switching (e.g. an interpreter alternating full sentences)
  works well; word-level switching within one sentence does not.
- **Very short utterances after a jingle/sting/BGM burst can misroute.**
  The language-switch guard (`--lang-switch-guard`, paired with
  `--lid-switch-confirm`) mitigates this but confidently-wrong LID+decode
  combinations (where the garbled text happens to match the wrong
  language's character set) remain a known blind spot -- see
  `docs/results/benchmarks.md`'s iteration #29 for a quantified before/after.
  `--lid-switch-confirm 1 --lang-switch-guard 0` fully disables the sticky
  hysteresis (every detection switches the session immediately), trading
  noise robustness for maximum responsiveness -- useful when validating
  whether the lock itself, rather than its tuning, is the right call for
  your setup.
- **A session's very first utterance always confirms against SenseVoice
  before deciding the language** (see `docs/eval/noise.md`'s dual-LID confirm
  section), so a whisper-tiny bootstrap misfire can no longer route the
  session to a language with no matching model. Languages outside
  SenseVoice's 5 (ja/en/zh/ko/yue) -- European/`--minimal`-uncovered ones --
  still need `--lang-switch-guard`-length segments repeated
  `--lid-switch-confirm` times to become the session's confirmed language,
  so a legitimate European-language session establishes with a short delay
  at startup rather than instantly.
- **`--hotwords` currently has no effect on the ja (ReazonSpeech) tier.**
  ReazonSpeech's `tokens.txt` is byte-level BPE, incompatible with the
  `modeling_unit=cjkchar` encoding Realtime Translator uses for hotwords, so every
  hotword fails to encode (sherpa-onnx only reports this as stderr warnings
  and still exits 0 -- see GitHub issue #1). Realtime Translator now prints a startup
  warning telling you how many hotwords failed to encode; use `--replace`
  for ja proper nouns instead. A real fix needs either a matching
  `bpe.model` for the ReazonSpeech release (not currently shipped) or a
  from-scratch byte-BPE hotword encoder -- tracked as future work.
- **Two overlapping speakers are not separated**, even after the refine
  pass -- pyannote segmentation-3.0 can flag overlap regions, but Realtime Translator
  doesn't do overlap-aware transcription (whichever speaker's turn is
  processed first wins). What the refine pass *does* do now: instead of
  a plain majority vote across a group's live-pass labels, each finalized
  group (silence gap or 25s length) is re-diarized with pyannote
  segmentation-3.0 + the same CAM++ embeddings `--speakers`'s live pass
  uses, and the resulting local clusters are remapped onto the session's
  global S{n} labels -- so a group containing several speaker turns keeps
  a separate `[refine/S{n}]` line per turn instead of collapsing to one.
  Measured on 5 AMI meetings (50 min total, CC BY 4.0, collar 0.25s; see
  `docs/design/diarization.md` section 8), this cut mean DER from 25.7%
  (live-pass-only labeling) to 13.9%. Reference speaker count is 4 per
  meeting; Realtime Translator's hypothesis still overestimates it (4-8 speakers),
  so `--speakers` should be read as good-enough turn labeling, not a
  reliable speaker-count source. To keep that overcount from flooding the
  screen with one-off labels, a speaker's first appearance shows as
  provisional (`S5?`) and only resolves to a plain `S{n}` once it recurs; a
  label that never recurs stays `?` for the rest of the session (see
  `docs/design/diarization.md` section 11).
- **Translation quality has a real ceiling**, not just a tuning one.
  FuguMT (ja->en) and M2M-100 (ja->zh/ko) are small models; repetition loops
  are suppressed but not eliminated, and numeric values are not reliably
  preserved in ja->zh/ko translation (see `docs/design/translate.md` and
  `docs/design/translate_m2m.md` for measured failure cases before you rely on this
  for anything numeric or financial).
- **Multi-sentence speech can still lose its leading sentence(s).** The
  offline recognizers sometimes collapse a buffer holding several utterances
  into a single decode. Realtime Translator watches for the symptom -- a transcript far
  shorter than the speech it is supposed to cover -- and retries that buffer
  split at its internal silences (at least 0.35s), keeping the retry only
  when it actually recovers text. Two classes escape this: sentences spoken
  back-to-back with no pause to split on (the clip-324 class from the
  head-dropout investigation), and milder dropouts whose transcript still
  looks long enough to pass for normal.
- **The end-to-end mic pipeline has not been independently verified beyond
  this project's own testing** -- see `docs/design/goals.md`'s remaining-work
  section. File an issue if your results differ from the numbers above.
- **The opt-in 4-class punctuation model (`--punct-model 4class`) never emits
  `！` by default.** It's fine-tuned on dense open-web text (FineWeb-2), and
  on speech its `！` head fires on emphatic narration as confidently as on
  real exclamations, so a probability threshold cannot separate them. The
  decode therefore gates `！` off (`exclaim_threshold > 1`) and gates `、` at
  0.8. With those defaults it beats the shipped model on sparse-punctuation
  ja TV captions too (`testdata/eval_real`: F1 0.64 vs 0.62) and on FLEURS ja
  (0.89 vs 0.62). Pass `PunctuatorJa4Class(exclaim_threshold=0.9)` for dense
  text where `！` matters. See `docs/eval/punct_retrain.md`, "Decode gates".
- **Transcribing one continuous utterance of about 20 seconds in a single pass drops runs of characters in the middle.** On 6 clips of 20 to 21 seconds (the evaluation clips above, joined 3 at a time), the CER of a single-pass decode was 38.5%, against 8.7% for the original clips decoded one by one (one measurement on 2026-10-22: `cargo test --release` on the tree of bdc071a; whether recognition ran on the GPU or the CPU was not recorded; not independently verified). The record is `docs/results/long_buffer/README.md` on the branch `feat/long-buffer` (59c078f), which is not on main. The design of the fix is in section 3.22 of `docs/design/rust_port.ja.md`; the implementation is on hold as part of the accuracy work.

## License

Source code is MIT (`LICENSE`, copyright oboroge0). No model weights are
committed to this repository -- `hayamimi models fetch` fetches them
from their original publishers at install time, and each carries its own
license (`THIRD_PARTY_NOTICES.md` has the full table).

**One model is not permissive:** the ja->en translation model
(`mojicast-fugumt-ja-en-ct2`, used by `--translate en`) is
**CC BY-SA 4.0 (share-alike)**. If you redistribute that model's weights,
you must keep attribution and license any redistribution under CC BY-SA 4.0
too. This does not affect Realtime Translator's own code license, and does not affect
any other `--translate` target (M2M-100, MIT).

## Credits

Realtime Translator exists on top of, and would not exist without:

- [k2-fsa/sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) -- the ONNX
  Runtime inference engine every model here runs through.
- [ReazonSpeech](https://research.reazon.jp/) (Reazon Human Interaction Lab)
  -- the Japanese ASR model that anchors this project's accuracy claim.
- [NVIDIA NeMo / Parakeet](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)
  -- English + 24 European languages.
- [Meta AI Omnilingual ASR](https://github.com/facebookresearch/omnilingual-asr)
  -- the ~1600-language fallback that makes "multilingual" not a lie.
- [FunASR / SenseVoice](https://github.com/FunAudioLLM/SenseVoice) (Alibaba
  DAMO Academy) -- Chinese, Korean, and Cantonese ASR.
- [Mojicast](https://github.com/ishiki-emo/mojicast) (ishiki-emo) -- design
  inspiration for the live-captioning pipeline, and the source of the
  converted punctuation/translation model artifacts this project uses.
  Mojicast is itself a full offline real-time captioning app worth checking
  out.
- [Silero VAD](https://github.com/snakers4/silero-vad) -- voice activity
  detection.
- [3D-Speaker](https://github.com/modelscope/3D-Speaker) (Alibaba DAMO
  Academy) -- the CAM++ speaker embedding model behind `--speakers`.
- [Kiwi](https://github.com/bab2min/kiwipiepy) -- Korean morphological
  tokenizer, used to fix SenseVoice's token-spaced Korean output.
- [SB Intuitions](https://huggingface.co/sbintuitions) --
  [modernbert-ja-30m](https://huggingface.co/sbintuitions/modernbert-ja-30m),
  the MIT base model behind the opt-in 4-class ja punctuation model
  (`--punct-model 4class`).
- [FineWeb-2](https://huggingface.co/datasets/HuggingFaceFW/fineweb-2)
  (Hugging Face) -- the `jpn_Jpan` training text (ODC-By 1.0) that model was
  fine-tuned on.

## Documentation

[`docs/README.md`](docs/README.md) is the index: an embedding guide, a tuning
reference listing every knob and the record behind its default, the current
accuracy numbers, and the dated experiment logs and design investigations
behind them.

## Contributing

See `CONTRIBUTING.md`.
