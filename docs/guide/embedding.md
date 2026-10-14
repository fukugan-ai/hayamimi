# Embedding hayamimi's Python engine in another app

Moved out of the repository root `README.md` on 2026-09-03; the two sections
below are verbatim, so "above" in them still means "earlier in the README"
([`../../README.md`](../../README.md)).

For the Flutter/Dart side, the embedding guide is
[`mobile/hayamimi_core/README.md`](../../mobile/hayamimi_core/README.md) --
this file is the Python engine only.

## Embedding in another app

`scripts/realtime_transcribe.py`'s pieces (`RoutedASR`, `build_vad`,
`run_stream`) are importable, not just CLI-only: `RoutedASR(...)` and
`build_vad(...)` raise a catchable `asr_engine.ModelUnavailable` instead of
letting sherpa-onnx's C++ layer call `exit()` on a missing model path, and
`run_stream(..., stop_event=some_threading_Event)` accepts a
`threading.Event` cancellation token so a host app running the pipeline on
its own thread can stop it cleanly (`stop_event.set()`) without relying on
`KeyboardInterrupt`, which only works for the CLI's own process.

**Shutting down a pipeline.** Stopping `run_stream` is not the same as
releasing the engine. `Refiner`, `TranslationWorker` and `RoutedASR` each
run background threads, and a running thread holds its owner alive -- so a
host app that merely dropped its references kept every sherpa-onnx
recognizer (several GB) resident for the life of the process, and building
a second pipeline added a second set. Call `close()` when a pipeline is
done, refiner first (it decodes through the engine):

```python
refiner.close()          # drains whatever refine work is still queued, then stops
translator_worker.close()
asr.close()              # joins the preload/prefetch threads, frees the models
```

All three are idempotent and usable as context managers. `close()` is the
end of that object's life, not a pause: `RoutedASR.transcribe()/partial()/
identify()` and `Refiner.add_span()/maybe_refine()` raise `RuntimeError`
afterwards, so build a new pipeline for a new session -- or, to start a
fresh conversation on the models already loaded, use
`realtime_transcribe.reset_live_session()` instead, which resets session
state and keeps every model resident. `main()` closes all three at
shutdown, after the final `session_summary` is published, so the event
stream an embedder sees is unchanged.

## Embedding: runtime control and structured events

Two gaps remained even with the pieces above importable. First, session
settings -- which language to force, whether to translate, how sensitive the
VAD (voice activity detector, the component that decides where an utterance
starts and ends) is -- could only be set once, at construction: a host app
whose user changed their mind mid-session had to tear down the whole engine
and rebuild it. Second, structured session events only existed at all behind
`--serve`: without it, the only way to learn that a model failed to load, or
that a segment's language just switched, was to scrape stderr and the
console's free-text diagnostic line, which a program can't parse reliably.

Both are addressed by the same mechanism. `realtime_transcribe.main()` (and
any app that constructs `subtitle_server.EventHub` directly) now always
creates an event hub, and every stage of the pipeline publishes to it
whether or not `--serve`'s HTTP server exists. `--serve` just adds that HTTP
server as an optional front end over the same hub, plus three JSON
endpoints backed by a `subtitle_server.RuntimeControls` bundle.

**Consuming events without HTTP.** An app importing this module directly
doesn't need `--serve`, or even a `SubtitleServer`, to see these events:
`hub.add_listener(callback)` registers a synchronous, in-process callback
invoked with the raw event dict on every publish. `hub.subscribe()` -- what
the dashboard/overlay pages above consume internally, over SSE at
`/events`, and what `scripts/ws_ingest.py` mirrors onto a WebSocket client
-- hands back a `queue.Queue` of the same events as JSON strings instead,
for a consumer that wants to poll rather than be called back.

**Event types.** Every event is a JSON object with a `type` key:

| `type` | Shape | When |
|---|---|---|
| `session_start` | `{"type":"session_start"}` | once, at the top of a session |
| `partial` | `{"type":"partial","text":str}` | an in-progress draft updates (about every 0.5s while speaking) |
| `final` | `{"type":"final","id":str,"source":str,"t0_s":float,"t1_s":float,"end_wall_ms":int,"text":str,"lang":str,"speaker":str,"latency_ms":float\|null,"tier":str,"audio_s":float,"lid_ms":float\|null,"decode_ms":float\|null,"switched":bool}` | a VAD segment finalizes; `switched` is true only when `lang` differs from the previous final's (false for the session's first) |
| `translation` | the shape in "The `translation` event" below | the translation relay sends a translation of a final, a refine line or a draft in progress; it goes out on a port of the relay's own, not on the `/events` of `hayamimi` |
| `speaker` | `{"type":"speaker","id":str,"speaker":str}` | the speaker of a final is decided after the final; see "The `speaker` event" below |
| `fallback` | `{"type":"fallback","stage":str,"reason":str,"to":str\|null}` | a stage did not run as designed and moved to another road; with `to` null it has returned to its first road and carries no `reason`. When the relay cannot reach an outside destination, `stage` is `sink` |
| `refine` | `{"type":"refine","text":str,"lang":str,"speaker":str,"audio_s":float}` | the second-pass re-decode of an utterance group lands |
| `model_load` | `{"type":"model_load","model":str,"phase":"start"\|"done","ms":float\|null}` | a recognizer/LID/punctuation/translator model starts or finishes loading; `model` is the engine's own short name (`rz`/`pz`/`sv`/`v3`/`omni`/`pja`/`lid`/`punct`, or `translator:<lang>`). `model` stays `"punct"` regardless of `punct_model=` -- to tell bert from 4class, watch for a `punct_model_unavailable` warning (its absence means the requested one loaded) |
| `model_fallback` | `{"type":"model_fallback","requested":str,"used":str,"reason":str}` | routing substituted a different tier because the one asked for is missing (e.g. a `--minimal` install), reported once per requested model per session |
| `warning` | `{"type":"warning","code":str,"message":str}` | a degraded-but-non-fatal condition; `code` is one of `hotwords_unencodable`, `segmentation_vad_unavailable`, `second_opinion_unavailable`, `diarization_failed`, `punct_model_unavailable` |
| `session_summary` | `{"type":"session_summary","stats":{...},"speakers":{...}\|null}` | at process shutdown and again right before a session reset; `stats` mirrors the `=== session summary: ... ===` console line's numbers, `speakers` mirrors the speaker-diagnostic console lines when `--speakers` is on (`null` otherwise) |
| `recluster` | `{"type":"recluster","time_s":float,"n_entries":int,"n_clusters":int,"mapping":{...}}` | `--speaker-global-recluster`'s end-of-session diagnostic actually runs |
| `session_reset` | `{"type":"session_reset"}` | a `POST /reset` (or a direct `reset_live_session()` call) finishes |

**Items that tie a line to its translation.**
The Rust `hayamimi` sends a `final` with five more items than the shape above.
The existing items do not change.

| Item | Type | Meaning |
|---|---|---|
| `id` | string | `<source>-<sequence number>`, counted from 1 for each start of the process |
| `source` | `"mic"`\|`"loopback"`\|`"file"` | Which input the final came from. It follows from `--input` and can be overridden with `--source` |
| `t0_s` | float | Start of the utterance, in seconds from the start of the stream |
| `t1_s` | float | End of the utterance, in seconds |
| `end_wall_ms` | integer | The time the VAD decided the utterance had ended (epoch ms) |

The translation relay (`hayamimi-relay`) joins a final to its translation by this `id`.
It joins by `id` because a translation that is pasted onto the previous line would land on the wrong line whenever it arrives after the next final.

**The `translation` event.**
The relay sends `translation` events on a port of its own, in this shape:

```json
{"type":"translation","id":str,"of":"final"|"refine"|"partial","source":str,"src_lang":str,"lang":str,
 "src_sha256":str,"stage":"t"|"e","settled":bool,
 "status":"ok"|"held","hold_reason":null|"number"|"negation"|"uncertainty"|"proper_noun"|"form"|"unavailable",
 "text":str,"annotations":[{"start":int,"end":int,"text":str}],
 "skipped":[{"kind":str,"reason":"ambiguous"|"no_rate"|"stale_rate"|"unplaced"}]}
```

- `settled` says whether this is the line's decided translation. A `translation` with `settled` false is a draft and can be replaced by a later one with the same `id`.
- A line with `of` set to `final` gets at most two: a draft (`stage` `t`, `settled` false), then one decided translation (`settled` true). A line with `of` set to `refine` gets the decided one only.
- A translation with `of` set to `partial` is a translation of the draft in progress. Its `stage` is `t`, `settled` is false and `status` is `ok`, and its `id` is the one the utterance's final will take. It is never passed on to an outside destination (OBS, Zoom, Teams).
- When `status` is `held`, `text`, `annotations` and `skipped` are empty. The text of a withheld translation is never put on the wire.
- `start` and `end` in `annotations` are positions in `text`, counted in Unicode scalar values from 0. The screen and the SRT draw each element's `text` inserted at the `end` position.
- `src_sha256` is the sha256 of the source text the translation was made from, in lowercase hex.

**The `speaker` event.**
A final does not wait for the speaker embedding, so the `speaker` of a `final` is an empty string.
Once the speaker of an utterance is decided, one `{"type":"speaker","id":<id of the final>,"speaker":<mark>}` follows with the same `id`.
It is not sent for an utterance whose mark stays undecided.
A build without speaker identification leaves `speaker` empty and does not send this event.

**Forms not taken.**
The design considered and rejected two forms:

- **Sending `translation` on the `/events` of `hayamimi`**: the relay takes the events of both recognizer processes in one place and joins them by `id`, so the `hayamimi` side does not change.
- **Making a final wait for the speaker embedding**: that would put about 70 ms of embedding on the thread that shows subtitles.

**Runtime control over HTTP (`--serve` only).** Three endpoints besides the
`/replacements`/`/itn_overrides` pair described above:

```bash
# read the live session's current configuration
curl http://localhost:8833/config

# force the session onto English, and stop requiring SenseVoice's own LID
# to agree before a language switch is accepted
curl -X POST http://localhost:8833/config \
  -d '{"lang": "en", "dual_confirm": false}'

# add Korean as a live translation target (any target NOT listed here is
# removed -- this replaces the whole set, like the other config keys)
curl -X POST http://localhost:8833/config -d '{"translate": ["en", "ko"]}'

# loosen VAD sensitivity: wait longer for silence before a segment finalizes
curl -X POST http://localhost:8833/config -d '{"vad": {"min_silence": 0.6}}'

# start a fresh conversation: forget every speaker and language-switch
# state, but keep every model resident (no reload cost)
curl -X POST http://localhost:8833/reset
```

`GET /config` returns `{"lang": null|str, "dual_confirm": bool, "punctuate":
bool, "lid_switch_confirm": int, "min_switch_s": float, "translate":
[str, ...], "vad": {"threshold": float, "min_silence": float, "max_speech":
float}}`. `POST /config` accepts any subset of those keys and applies each
through the matching `RoutedASR`/`TranslatorPool`/VAD setter; an invalid
value (an unroutable language code, a negative `min_switch_s`, an
unsupported translation target) answers `400` with that setter's own error
message rather than applying a partial or silently-clamped change. VAD
sensitivity is the one key that can't take effect immediately: sherpa-onnx
has no in-place setter for it, so a `vad` change is deferred until the
detector isn't in the middle of a speech segment and every completed segment
has been drained, then it rebuilds. Requests waiting for that boundary compose
field by field: setting `threshold`, then only `min_silence`, keeps both changes;
the latest explicit value wins for a repeated field. An empty or all-null VAD
update does nothing. `GET /config` reports the active settings until the rebuild
succeeds. Rebuilding preserves absolute segment positions, so the audio history
and refine pass continue to refer to the same samples.

`POST /reset` clears the running session's speaker centroids, sticky/pending
language state, and refine/session statistics, without reloading any model
-- the next segment starts as if the process had just launched. Useful
between unrelated recordings inside one long-running process (a stream host
switching guests, a kiosk resetting between visitors), where paying the
model-load cost again would be wasted time.

The reset itself doesn't run on the HTTP handler thread: it has to run on
the same thread that's decoding audio, since it touches speaker/language
state that thread reads and writes with no locking of its own (running it
directly from a second thread was found, in review, to be able to raise an
exception on the decode thread mid-segment). It's queued instead, and picked
up the next time that thread reaches a safe point between audio chunks --
normally near-instant, since a chunk is only tens of milliseconds. `POST
/reset` answers `200 {"ok": true}` once the reset has actually run, or, if
ten seconds pass without a safe point to apply it at, `202 {"ok": false,
"pending": true}` -- it's still queued and will still apply, just later.
This matters mainly for `--input ws`: with no client currently sending
audio, nothing is reaching those chunk boundaries at all, so a reset
requested then won't apply until audio resumes.
