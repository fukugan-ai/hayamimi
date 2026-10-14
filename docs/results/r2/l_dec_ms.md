# R2 の `l_dec_ms`（`--wav` の lat-ja）

`l_dec_ms` は、VAD が区切りを渡してから、ITN、句読点、置換辞書を終えた文字ができるまでの時間である。
`asr.utterance.timed` の行に 1 発話ごとに出る。
この値は合否に使わない。
`/ingest` が無いので、取り込みから字幕までの `L` と `L_asr` は測っていない。

## 結果

| OS | run | 標本 | p50（ms） | p95（ms） | min（ms） | max（ms） |
|---|---|---|---|---|---|---|
| WSL | 1 | 26 | 161.3 | 336.0 | 85.9 | 964.7 |
| WSL | 2 | 26 | 166.0 | 292.4 | 101.4 | 527.9 |
| WSL | 1 と 2 | 52 | 166.0 | 336.0 | 85.9 | 964.7 |
| Windows | 1 | 26 | 251.8 | 419.7 | 134.4 | 524.9 |
| Windows | 2 | 26 | 224.9 | 356.7 | 126.1 | 536.0 |
| Windows | 1 と 2 | 52 | 235.7 | 419.7 | 126.1 | 536.0 |
| Python（記録） | `gpu_stages.md` の `final` の `latency_ms`（`L_dec`） | 199 | 311 | 446 | | |

## 読み方

p50 と p95 は nearest-rank で、`hayamimi_eval::percentile::nearest_rank` と同じ式（昇順に並べた標本の `ceil(n*p)` 番目）を `jq` で当てた。
1 回の run は lat-ja 全体（約 194 秒）を音声の速さで流し、26 発話が確定した。

Rust の `--min-silence` の既定は 0.30 秒（設計の H9）で、Python の記録は 0.35 秒である。
区切りの位置が違うので、同じ発話どうしの比較にはならない。
標本の数も違う（Python は 199、Rust は 52）。

WSL の run 1 の max 964.7 ms は 1 発話である。
run の間は他の process が動いていて、`uptime` の負荷平均は 1.7 から 2.8 だった。
負荷を揃えた測定ではない。

Windows の p50 が WSL より 70 ms ほど大きい。
原因は特定していない（NOT VERIFIED）。
切り分けるなら、同じ機械で句読点を CPU に固定した run と GPU の run を並べる。

## 打った command

```bash
hayamimi --wav testdata/lat-ja.wav --mode single --lang ja --models models > docs/results/r2/wsl/lat_ja_l_dec_1.jsonl
jq -s -r '[.[]|select(.event=="asr.utterance.timed")|.l_dec_ms]|sort as $v|($v|length) as $n|"n=\($n) p50=\($v[(($n*0.5)|ceil)-1]) p95=\($v[(($n*0.95)|ceil)-1])"' docs/results/r2/wsl/lat_ja_l_dec_*.jsonl
```

Windows は同じ引数で `hayamimi.exe` を流し、出力を `docs/results/r2/win/` に置いた。
