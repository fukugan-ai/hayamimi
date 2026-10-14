# hayamimi requirements

[日本語](requirements.ja.md)

This English document is the canonical version; the Japanese version is a translation.
It gathers requirements that are decided elsewhere and gives the source of each row.
It decides nothing new.
The source documents hold the decisions, the reasons and the measurements, and they win if this document disagrees with them.
Implementation progress is in [status.md](status.md).

Source abbreviations:

- **G**: `docs/design/goals.md`
- **D**: `docs/design/rust_port.ja.md` (acceptance IDs `RP-<n>`)
- **C**: `docs/design/conversation_subtitles.ja.md` (acceptance IDs `CS-<n>` and `CX-<n>`)
- **S**: `goal` and `targets` in `state.json` at the repository root (a working record kept outside git; it is not in the public repository)

## 1. Purpose

Shorten the time from speaking to seeing the translated subtitle, compared with the current Python version (D opening, D 1.1).
In a Japanese–English conversation, let each person read the other's speech as subtitles in their own language, overlaid on meetings, videos and streams, and delivered to the other side as well (C opening, S `goal`).
Take Python out of the run-time path, rewrite it in Rust, and run inference on the GPU; without a GPU, run on the CPU (D opening, D 3.11).

## 2. User scenarios

| Scenario | What is needed | Source |
|---|---|---|
| A meeting between a Japanese speaker and an English speaker (Zoom, Teams) | Translate the other side's English into Japanese and your own Japanese into English, show both in one overlay window, and send the translation to the meeting's captions | C opening, C 1, S `targets` |
| Watching a video (YouTube, MPC-HC) | Turn the audio from the speakers into subtitles over the screen | C opening, C 3.1 |
| Subtitles for a video file | Write `<name>.<lang>.srt` into the same directory | C 1, C 3.13, S `targets` |
| Streaming (OBS) | Overlay the English translation of your own speech on the video as a transparent caption page | C 1, D 3.8 |
| Transcribing alone | Recognize audio from the microphone, loopback, `/ingest` or a wav file as it arrives, and emit final sentences | G decisions, D 3.7 |

## 3. Functional requirements

| ID | Requirement | Source | Acceptance |
|---|---|---|---|
| FR-1 | Take audio through five inputs: microphone, speaker loopback, a mix of the two, the `/ingest` WebSocket, and wav | D 3.7 | RP-10, RP-11 |
| FR-2 | Split utterances with VAD, identify the language, and emit final and partial text with a recognizer per language, centred on Japanese and English and covering the same languages as the current Python (ja, en, zh, ko, yue and others) | G decisions, D 1.2, D 3.5 | RP-1, RP-2, RP-5 |
| FR-3 | Apply punctuation, ITN (kanji numerals to Arabic numerals) and the replacement dictionary to Japanese final text | D 1.2, D 3.5 | RP-2, RP-18 |
| FR-4 | Label speakers in partial results and relabel them in the refined pass | D 1.2, D 3.5 | RP-15 |
| FR-5 | Translate final text with a language model (llama-server), make it natural, and verify it against the source; show the T translation first and replace it with the settled translation | D 3.6, C 3.4 | RP-3, RP-13, RP-25, CS-1 to CS-5, CS-25 to CS-27 |
| FR-6 | Translate partial results and draw only the settled prefix in the settled style | D 3.6 | RP-5, RP-25 |
| FR-7 | Add, in parentheses, currency and unit values converted to the reader's local units | C opening, C 3.11 | CX-1 to CX-11 |
| FR-8 | The subtitle server emits the same routes, queries and event shapes as the current Python | D 3.8 | RP-12 |
| FR-9 | Serve the dashboard, OBS overlay and transcript pages in Dioxus web, and show the conversation-subtitle overlay window | D 3.8, C 3.12 | RP-12, CS-10 to CS-13 |
| FR-10 | Send settled translations to Zoom's caption API and Teams CART | C 3.10 | CS-17, CS-18 |
| FR-11 | Write SRT from a video file into the same directory | C 3.13 | CS-14, CS-15 |
| FR-12 | Fetch models without Python and record their sha256 | D 3.10 | RP-21 |
| FR-13 | Switch log `message`, errors, CLI text and screen text by locale (English by default) | D 3.14 | RP-26 |
| FR-14 | Leave no Python in the run-time path; the 78 Python files are removed in R11 | D 1.3, D 3.2 | RP-9, RP-19 |

## 4. Non-functional requirements

| ID | Requirement | Source | Acceptance |
|---|---|---|---|
| NFR-1 | Speed first, quality second; where they conflict, speed wins | D 1.1, G decisions | NFR-2 to NFR-4 |
| NFR-2 | The main metric is the delay `L` from the end of an utterance to its translated subtitle, at p50 and p95. Rust's `L` is at most Python's, and Rust wins on at least 20/23 of the utterances in the comparison set. For en→ja, p95 is at most 2000 ms | D 1.1, D 3.1, D 6 | RP-3, RP-4 |
| NFR-3 | Recognition error rate, translation chrF and speaker DER are no worse than the current Python | D 1.3, D 6 | RP-1, RP-2, RP-13, RP-15 |
| NFR-4 | Inference is placed on the GPU by default; every stage kept on the CPU carries the evidence required by D 3.5 | D 1.4, D 3.5 | RP-6, RP-7, RP-8, RP-20, RP-23, RP-27 |
| NFR-5 | Never stop because something is missing. When the GPU, GPU memory, a model file, the inference library, llama-server, an external destination, a port, a capture device or a settings file is missing, fall back per stage to the CPU, a smaller model or a reduced form, and report each fallback as an event and on screen. `L` on the CPU fallback is measured apart from the GPU pass criteria | D 3.11, D 3.15 | RP-16, RP-28, RP-29 |
| NFR-6 | Peak GPU memory is at most 14,000 MiB | D 6 | RP-22 |
| NFR-7 | The self-built CUDA sherpa-onnx can be built with the same versions on Windows (msvc) and WSL | D 3.12 | RP-17, RP-24 |
| NFR-8 | Logs go to stdout as JSONL and never contain text, audio or paths | D 3.13 | RP-6, RP-16, RP-29 |
| NFR-9 | Translation flicker is counted, and the display does not change after the settled translation appears | D 3.6 | RP-25 |
| NFR-10 | Conversation audio and text are never sent to a cloud service | D 1.3, C 4 | No machine acceptance; held by the "not built" list in D 1.3 |

## 5. Means not taken

The main rejected means that shaped these requirements.
The full reasons are in the source sections.

| Not taken | Reason in brief | Source |
|---|---|---|
| Keep Python and call it from Rust | Python stays in the run-time path, so nothing is replaced | D 4, C 4 |
| Cloud recognition and translation APIs | Conversation audio and text would leave the machine | D 4, C 4 |
| A CPU translator (FuguMT, M2M-100) in place of the GPU language model | The quality ceiling is in the model | D 4, C 4 |
| Stop at start-up with a reason when something is missing | One missing piece would stop all subtitles | D 4 |
| Fall back to the CPU or a smaller model silently | Nobody could see why it became slower or less accurate | D 4 |
| Wait for both T and E translations before showing anything | Translation delay doubles | D 3.6, C 4 |
| Count the time of fragment translations in the main metric | Shorter sources look faster, and `L` can turn negative | D 4 |
| An end-to-end speech translation model | None runs both Japanese–English directions with an acceptable licence and a Rust path | D 4 |
| Rebuild mobile/ | Out of the request; wire-format compatibility is enough | D 4 |

## 6. Out of scope

| Out of scope | Source |
|---|---|
| Rebuilding mobile/ (Flutter); only wire-format compatibility is kept | D 1.3 |
| Sending to YouTube Live caption ingestion | C 1 |
| Overwriting the source value with the converted value | C 1 |
| Rust versions of the training and model-conversion tools | D 4 |
| Calling it "replaced" while accuracy or speed is worse than now | D 1.3 |

## 7. Requirements and acceptance IDs

| Acceptance ID | Requirement ID |
|---|---|
| RP-1 | FR-2, NFR-3 |
| RP-2 | FR-2, FR-3, NFR-3 |
| RP-3, RP-4 | FR-5, NFR-2 |
| RP-5 | FR-2, FR-6 |
| RP-6 | NFR-4, NFR-8 |
| RP-7, RP-8, RP-20, RP-23, RP-27 | NFR-4 |
| RP-9, RP-19 | FR-14 |
| RP-10, RP-11 | FR-1 |
| RP-12 | FR-8, FR-9 |
| RP-13 | FR-5, NFR-3 |
| RP-14 | NFR-3 (correctness of chrF scoring) |
| RP-15 | FR-4, NFR-3 |
| RP-16, RP-29 | NFR-5, NFR-8 |
| RP-28 | NFR-5 |
| RP-17, RP-24 | NFR-7 |
| RP-18 | FR-3 |
| RP-21 | FR-12 |
| RP-22 | NFR-6 |
| RP-25 | FR-5, FR-6, NFR-9 |
| RP-26 | FR-13 |
| CS-1 to CS-30, CX-1 to CX-11 | FR-5, FR-7, FR-9, FR-10, FR-11 (table in C 6.2) |

## 8. Open items

| Open item | Where it is decided |
|---|---|
| The translation model and the model of the naturalizing stage | The procedure in C 9 (measured on held-out data) |
| 2000 ms in RP-3, 20% in RP-7, 14,000 MiB in RP-22, and the CPU-fallback bounds in RP-28 | H8 and H8b in D 10; owners R7, R3 and R8e |
| The GPU memory thresholds in RP-6 (256 MiB and 64 MiB) | H8c in D 10 |
| Whether to adopt 0.25 for `--min-silence` | H9 in D 10; R7 |
| Whether C's CS-15 (stop before extraction when an SRT already exists) should follow D 3.15's never-stop form | A council for the owner of C |
| The English version `rust_port.md` | D opening; R10 |
