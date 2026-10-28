# IP-7 record

実走の条件（2026-10-23、commit `6eaf1c2`）：`ip07-fbcc99524162780c --ignored` を `cargo` の親 process なしで、`sudo -n nice -n -5` を付けて直に起こした。訳の server は 18084 で別に起こし、共有の 18081 は触れていない。
結果：`test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 126.69s`（T-IP-07-001、-002、-005、-006、-007 が通った）。
`ip07_pair.jsonl` は 60 行、`stream.machine.refused` は 0 件、`final` は 44 件。
以下は test が書いた記録（逐語）である。

## pair
mixed-up utterances: 1 (線 6) 線の内
utterances: 20
ja read as en: 1
en read as ja: 0
mixed up toward a third language: 0
finals in a third language: 0
utterances split into two finals or more: 0
repeats with no final: 16
utterances with no final: [1, 5, 7, 14]
translations into their own language: 0
asr.lang.fallback sv_tag: 14
asr.lang.fallback script: 0
asr.lang.fallback default: 0
asr.lang.fallback sv_failed: 0
fallback lines tied to a final: 14
asr.segment.unread empty_decode: 0
asr.segment.unread empty_text: 0
utterances with no segment: [1, 5, 7, 14]
utterances read as nothing: []
