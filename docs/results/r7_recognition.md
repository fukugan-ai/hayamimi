# R7 認識の置き場と 2 音源同時の遅れ

## 元にした run file

- `docs/results/r7/cpu_peers0.jsonl`
- `docs/results/r7/cpu_peers1.jsonl`
- `docs/results/r7/cuda_p0_recognize.jsonl`
- `docs/results/r7/cuda_p0_encoder_only.jsonl`
- `docs/results/r7/cuda_p1_recognize.jsonl`
- `docs/results/r7/cuda_p1_encoder_only.jsonl`

取り方（CUDA は認識と encoder 単独を別 process にする。peers は 0 と 1）：

```
hayamimi-eval stages --provider cuda --wav <wav> --models <models> --repeat 5 --peers <n> --only recognize --out <run>
hayamimi-eval stages --provider cuda --wav <wav> --models <models> --repeat 5 --peers <n> --only encoder_only,search_replay --out <run>
hayamimi-eval stages --provider cpu --wav <wav> --models <models> --repeat 5 --peers <n> --out <run>
hayamimi-eval r7-report --runs <run...> --conditions <file> --out <this document>
```

## run

| provider | peers | peer が終えた decode | repeat | 発話数 | os |
|---|---|---|---|---|---|
| cpu | 0 | 0 | 5 | 23 | linux |
| cpu | 1 | 604 | 5 | 23 | linux |
| cuda | 0 | 0 | 5 | 23 | linux |
| cuda | 1 | 644 | 5 | 23 | linux |

## 認識の時間

単位は µs。認識全体は create_stream から get_result まで、decode は探索を含む。

| provider | variant | peers | 認識全体 p50 | p95 | decode p50 | p95 | fbank p50 | p95 |
|---|---|---|---|---|---|---|---|---|
| cpu | fp32 | 0 | 349196 | 774595 | 339223 | 760268 | 10898 | 24475 |
| cpu | int8 | 0 | 316423 | 565974 | 302622 | 553026 | 11950 | 33229 |
| cpu | fp32 | 1 | 487816 | 922580 | 463965 | 906075 | 13553 | 31355 |
| cpu | int8 | 1 | 396586 | 757781 | 381951 | 741664 | 13620 | 34174 |
| cuda | fp16 | 0 | 614462 | 992889 | 606744 | 978613 | 6774 | 10530 |
| cuda | fp32 | 0 | 225569 | 401547 | 216883 | 390040 | 6311 | 12112 |
| cuda | fp16 | 1 | 1111784 | 1900299 | 1101171 | 1884902 | 10748 | 18600 |
| cuda | fp32 | 1 | 360316 | 708653 | 349247 | 696197 | 8731 | 24791 |

## 認識の置き場

peers が 1 の run の認識全体の p50（µs）。

| 比べる対 | GPU p50 | CPU p50 | 判定 |
|---|---|---|---|
| GPU fp32 対 CPU fp32 | 360316 | 487816 | CPU が GPU より遅い。認識の置き場を評議へ戻す |
| GPU fp32 対 CPU int8 | 360316 | 396586 | CPU が GPU より遅い。認識の置き場を評議へ戻す |

## CPU の段

単位は µs。CPU の run の、GPU の経路が無い段。

| 段 | peers | p50 | p95 |
|---|---|---|---|
| itn | 0 | 4 | 6 |
| itn | 1 | 4 | 6 |
| judge | 0 | 1 | 2 |
| judge | 1 | 1 | 2 |
| punct_rules | 0 | 2 | 2 |
| punct_rules | 1 | 2 | 2 |
| resample | 0 | 2871 | 6444 |
| resample | 1 | 3304 | 7990 |
| ring | 0 | 28 | 51 |
| ring | 1 | 33 | 75 |
| speaker_embed | 0 | 91198 | 142783 |
| speaker_embed | 1 | 167843 | 307712 |
| vad | 0 | 49617 | 82068 |
| vad | 1 | 60236 | 108894 |

## 探索の内訳

単位は µs。見積りの下は joiner を frame ごと、decoder を token ごとに呼ぶ時間、上は decoder も frame ごとに呼ぶ時間。

| provider | variant | frame 数 p50 | token 数 p50 | joiner 1 回 p50 | decoder 1 回 p50 | 探索の見積り 下 | 上 | 残り（decode − encoder − fbank） | 残りと見積り |
|---|---|---|---|---|---|---|---|---|---|
| cpu | fp32 | 146 | 28 | 141 | 256 | 27731 | 57909 | 172660 | 見積りより大きい。差は ort の外（原因は未特定） |
| cpu | int8 | 146 | 28 | 102 | 267 | 22327 | 53827 | - | encoder を単独で測っていない |
| cuda | fp32 | 146 | 28 | 336 | 519 | 63611 | 124832 | 136546 | 見積りより大きい。差は ort の外（原因は未特定） |


## 読み方

- peers が 1 の run は、別 process の 2 つ目の音源が run の間ずっと decode していた。peers が 0 の run は音源 1 つだけ。
- 同じ peers の cuda と cpu を比べる。run を取った時の機械の負荷（他の席の GPU 利用と CPU 負荷）は run file に載らない。比べるときは同じ時間帯に続けて取った run を使い、負荷は測定時に別に記録する。
- 探索の見積りは ort で joiner と decoder を同じ形の入力で呼んだ時間であって、sherpa-onnx の中の時間ではない。残りが見積りの上を超えるとき、差の原因は未特定である。

## 測定時の条件

- 機械：RTX 5060 Ti 16 GB。ほかの席の仕事と同居していた。load average（1 分）は、CPU の run の間は 16 から 32、CUDA の run の間は 10 から 25（`uptime`）。
- CUDA の run の前後とも、`nvidia-smi --query-compute-apps` にほかの 3 process が居た（WSL では使用量が N/A）。
- CPU の run と CUDA の run は、同じ時間帯に続けては取っていない。認識の置き場の判定（CPU が GPU より遅い）は、この負荷の下の値であり、取り直すまで確定とは読まない（NOT VERIFIED）。
- CUDA では `recognize` と `encoder_only,search_replay` を別 process で取った。同じ process では ort が DefaultLogger の error で落ちる。
- 探索の内訳は、ort で joiner と decoder を呼んだ時間である。sherpa-onnx の内部の時間ではなく、H31 の原因は未特定。
