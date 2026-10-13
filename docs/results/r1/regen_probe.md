# 段ごとの GPU と CPU の時間

## 測った機械と build

| 項目 | 値 |
|---|---|
| os | windows |
| cpu | AMD Ryzen 7 5700X |
| gpu | NVIDIA GeForce RTX 5060 Ti |
| driver | 610.88 |
| sherpa-onnx | v1.13.8 (11afbd009a7f8c08f4bcf2fc1b265d0df4670fbf) |
| onnxruntime | 1.31.0 |
| cuda | 13.4.92 |
| cudnn | 9.27.0 |
| build.json sha256 | ec49b57f09f3c1498b141936ff1f955f8484071f7f9aa2cd32496f6fcc96c20e |

## 段ごとの時間

単位は µs（1 µs 未満は 1 に切り上げる）。標本は発話数 × 繰り返し。負荷は、GPU の llama-server へ訳の依頼を送り続ける 2 本の client。

| 段 | provider | variant | 負荷 | run | 標本数 | p50_us | p95_us | 備考 |
|---|---|---|---|---|---|---|---|---|
| 音声の取り込み（WASAPI のマイク、loopback） | cuda | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | Rust の取り込み（WASAPI）がまだ無く、測れない |
| 音声の取り込み（WASAPI のマイク、loopback） | cuda | - | 負荷 | 0 | 0 | 未測定 | 未測定 | Rust の取り込み（WASAPI）がまだ無く、測れない |
| 音声の取り込み（WASAPI のマイク、loopback） | cpu | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | Rust の取り込み（WASAPI）がまだ無く、測れない |
| 音声の取り込み（WASAPI のマイク、loopback） | cpu | - | 負荷 | 0 | 0 | 未測定 | 未測定 | Rust の取り込み（WASAPI）がまだ無く、測れない |
| int16 → float32、16 kHz への resample、mix | cuda | transfer-floor | 無負荷 | 1 | 115 | 339 | 442 | 下限: 転送の往復だけで kernel を含まない。入力は 16 kHz を 48 kHz にした物。mono |
| int16 → float32、16 kHz への resample、mix | cuda | transfer-floor | 無負荷 | 2 | 115 | 339 | 447 | 下限: 転送の往復だけで kernel を含まない。入力は 16 kHz を 48 kHz にした物。mono |
| int16 → float32、16 kHz への resample、mix | cuda | transfer-floor | 無負荷 | 3 | 115 | 359 | 455 | 下限: 転送の往復だけで kernel を含まない。入力は 16 kHz を 48 kHz にした物。mono |
| int16 → float32、16 kHz への resample、mix | cuda | transfer-floor | 負荷 | 1 | 115 | 362 | 552 | 下限: 転送の往復だけで kernel を含まない。入力は 16 kHz を 48 kHz にした物。mono |
| int16 → float32、16 kHz への resample、mix | cuda | transfer-floor | 負荷 | 2 | 115 | 375 | 481 | 下限: 転送の往復だけで kernel を含まない。入力は 16 kHz を 48 kHz にした物。mono |
| int16 → float32、16 kHz への resample、mix | cpu | cpu | 無負荷 | 1 | 115 | 2132 | 4301 | 入力は 16 kHz を 48 kHz にした物。mono |
| int16 → float32、16 kHz への resample、mix | cpu | cpu | 無負荷 | 2 | 115 | 2779 | 5441 | 入力は 16 kHz を 48 kHz にした物。mono |
| int16 → float32、16 kHz への resample、mix | cpu | cpu | 負荷 | 1 | 115 | 3170 | 5667 | 入力は 16 kHz を 48 kHz にした物。mono |
| int16 → float32、16 kHz への resample、mix | cpu | cpu | 負荷 | 2 | 115 | 3220 | 5675 | 入力は 16 kHz を 48 kHz にした物。mono |
| 音声の ring buffer と preroll（1.0 秒） | cuda | transfer-floor | 無負荷 | 1 | 115 | 247 | 294 | 下限: 転送の往復だけで kernel を含まない。preroll 1.0 秒の取り出しを含む |
| 音声の ring buffer と preroll（1.0 秒） | cuda | transfer-floor | 無負荷 | 2 | 115 | 267 | 339 | 下限: 転送の往復だけで kernel を含まない。preroll 1.0 秒の取り出しを含む |
| 音声の ring buffer と preroll（1.0 秒） | cuda | transfer-floor | 無負荷 | 3 | 115 | 260 | 311 | 下限: 転送の往復だけで kernel を含まない。preroll 1.0 秒の取り出しを含む |
| 音声の ring buffer と preroll（1.0 秒） | cuda | transfer-floor | 負荷 | 1 | 115 | 770 | 1949 | 下限: 転送の往復だけで kernel を含まない。preroll 1.0 秒の取り出しを含む |
| 音声の ring buffer と preroll（1.0 秒） | cuda | transfer-floor | 負荷 | 2 | 115 | 553 | 1795 | 下限: 転送の往復だけで kernel を含まない。preroll 1.0 秒の取り出しを含む |
| 音声の ring buffer と preroll（1.0 秒） | cpu | cpu | 無負荷 | 1 | 115 | 15 | 27 | preroll 1.0 秒の取り出しを含む |
| 音声の ring buffer と preroll（1.0 秒） | cpu | cpu | 無負荷 | 2 | 115 | 18 | 37 | preroll 1.0 秒の取り出しを含む |
| 音声の ring buffer と preroll（1.0 秒） | cpu | cpu | 負荷 | 1 | 115 | 18 | 32 | preroll 1.0 秒の取り出しを含む |
| 音声の ring buffer と preroll（1.0 秒） | cpu | cpu | 負荷 | 2 | 115 | 22 | 43 | preroll 1.0 秒の取り出しを含む |
| VAD（Silero、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 1 | 115 | 359090 | 558108 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| VAD（Silero、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 2 | 115 | 408326 | 637932 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| VAD（Silero、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 3 | 115 | 398069 | 612224 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| VAD（Silero、sherpa-onnx の `provider`） | cuda | fp32 | 負荷 | 1 | 115 | 1162339 | 1825923 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| VAD（Silero、sherpa-onnx の `provider`） | cuda | fp32 | 負荷 | 2 | 115 | 1078869 | 1782773 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| VAD（Silero、sherpa-onnx の `provider`） | cpu | fp32 | 無負荷 | 1 | 115 | 38584 | 61305 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| VAD（Silero、sherpa-onnx の `provider`） | cpu | fp32 | 無負荷 | 2 | 115 | 40455 | 66862 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| VAD（Silero、sherpa-onnx の `provider`） | cpu | fp32 | 負荷 | 1 | 115 | 40226 | 66936 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| VAD（Silero、sherpa-onnx の `provider`） | cpu | fp32 | 負荷 | 2 | 115 | 43863 | 73432 | 発話の後ろに 0.5 秒の無音を足して区切りを出すまで |
| 言語判定（whisper-tiny） | cuda | fp32 | 無負荷 | 1 | 115 | 71702 | 102797 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cuda | fp32 | 無負荷 | 2 | 115 | 78547 | 98642 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cuda | fp32 | 無負荷 | 3 | 115 | 76400 | 95108 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cuda | fp32 | 負荷 | 1 | 115 | 94216 | 117386 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cuda | fp32 | 負荷 | 2 | 115 | 92956 | 123410 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cpu | fp32 | 無負荷 | 1 | 115 | 131643 | 157347 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cpu | int8 | 無負荷 | 1 | 115 | 120017 | 140041 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cpu | fp32 | 無負荷 | 2 | 115 | 152465 | 169124 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cpu | int8 | 無負荷 | 2 | 115 | 127939 | 148304 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cpu | fp32 | 負荷 | 1 | 115 | 159365 | 182179 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cpu | int8 | 負荷 | 1 | 115 | 130517 | 148030 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cpu | fp32 | 負荷 | 2 | 115 | 161288 | 188849 | 先頭 4 秒を渡す |
| 言語判定（whisper-tiny） | cpu | int8 | 負荷 | 2 | 115 | 134462 | 159719 | 先頭 4 秒を渡す |
| 認識の encoder（全 model） | cuda | fp16 | 無負荷 | 1 | 115 | 468022 | 691439 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp32 | 無負荷 | 1 | 115 | 169531 | 238592 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp16 | 無負荷 | 2 | 115 | 465592 | 716097 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp32 | 無負荷 | 2 | 115 | 175123 | 242757 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp16 | 無負荷 | 3 | 115 | 445120 | 665899 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp32 | 無負荷 | 3 | 115 | 169589 | 254209 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | ort-encoder fp32 | 無負荷 | 1 | 115 | 69956 | 81699 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | ort-encoder fp32 | 無負荷 | 2 | 115 | 67306 | 81751 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | ort-encoder fp32 | 無負荷 | 3 | 115 | 61524 | 72805 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp16 | 負荷 | 1 | 115 | 888447 | 1337074 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp32 | 負荷 | 1 | 115 | 384211 | 529985 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp16 | 負荷 | 2 | 115 | 830152 | 1346456 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | fp32 | 負荷 | 2 | 115 | 366142 | 577796 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | ort-encoder fp32 | 負荷 | 1 | 115 | 89869 | 151853 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cuda | ort-encoder fp32 | 負荷 | 2 | 115 | 96003 | 167002 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | fp32 | 無負荷 | 1 | 115 | 149733 | 221815 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | int8 | 無負荷 | 1 | 115 | 125202 | 190405 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | fp32 | 無負荷 | 2 | 115 | 152865 | 224179 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | int8 | 無負荷 | 2 | 115 | 132028 | 209596 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | ort-encoder fp32 | 無負荷 | 1 | 115 | 121313 | 189086 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | ort-encoder fp32 | 無負荷 | 2 | 115 | 125860 | 192107 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | ort-encoder fp32 | 無負荷 | 3 | 115 | 116100 | 176989 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | fp32 | 負荷 | 1 | 115 | 167345 | 244386 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | int8 | 負荷 | 1 | 115 | 146669 | 218058 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | fp32 | 負荷 | 2 | 115 | 182705 | 286025 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | int8 | 負荷 | 2 | 115 | 156628 | 238802 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | ort-encoder fp32 | 負荷 | 1 | 115 | 113373 | 181256 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| 認識の encoder（全 model） | cpu | ort-encoder fp32 | 負荷 | 2 | 115 | 122139 | 182994 | variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない） |
| fbank と探索（sherpa-onnx の中） | cuda | fp16 | 無負荷 | 1 | 115 | 5798 | 12383 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp32 | 無負荷 | 1 | 115 | 5787 | 11526 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp16 | 無負荷 | 2 | 115 | 5586 | 12046 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp32 | 無負荷 | 2 | 115 | 6038 | 12492 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp16 | 無負荷 | 3 | 115 | 5333 | 11086 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp32 | 無負荷 | 3 | 115 | 5749 | 10976 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp16 | 負荷 | 1 | 115 | 6679 | 12924 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp32 | 負荷 | 1 | 115 | 5988 | 11368 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp16 | 負荷 | 2 | 115 | 6942 | 13217 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cuda | fp32 | 負荷 | 2 | 115 | 6725 | 12359 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cpu | fp32 | 無負荷 | 1 | 115 | 7361 | 11498 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cpu | int8 | 無負荷 | 1 | 115 | 7229 | 11593 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cpu | fp32 | 無負荷 | 2 | 115 | 7589 | 12168 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cpu | int8 | 無負荷 | 2 | 115 | 7826 | 13279 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cpu | fp32 | 負荷 | 1 | 115 | 7782 | 13273 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cpu | int8 | 負荷 | 1 | 115 | 8101 | 13954 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cpu | fp32 | 負荷 | 2 | 115 | 9044 | 14903 | fbank だけ。探索は encoder と分けられず上の行に含む |
| fbank と探索（sherpa-onnx の中） | cpu | int8 | 負荷 | 2 | 115 | 8674 | 14348 | fbank だけ。探索は encoder と分けられず上の行に含む |
| 冒頭欠落の疑い判定と分割再試行 | cuda | transfer-floor | 無負荷 | 1 | 115 | 216 | 283 | 下限: 転送の往復だけで kernel を含まない。判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| 冒頭欠落の疑い判定と分割再試行 | cuda | transfer-floor | 無負荷 | 2 | 115 | 241 | 301 | 下限: 転送の往復だけで kernel を含まない。判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| 冒頭欠落の疑い判定と分割再試行 | cuda | transfer-floor | 無負荷 | 3 | 115 | 240 | 312 | 下限: 転送の往復だけで kernel を含まない。判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| 冒頭欠落の疑い判定と分割再試行 | cuda | transfer-floor | 負荷 | 1 | 115 | 452 | 1791 | 下限: 転送の往復だけで kernel を含まない。判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| 冒頭欠落の疑い判定と分割再試行 | cuda | transfer-floor | 負荷 | 2 | 115 | 939 | 1953 | 下限: 転送の往復だけで kernel を含まない。判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| 冒頭欠落の疑い判定と分割再試行 | cpu | cpu | 無負荷 | 1 | 115 | 1 | 1 | 判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| 冒頭欠落の疑い判定と分割再試行 | cpu | cpu | 無負荷 | 2 | 115 | 1 | 1 | 判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| 冒頭欠落の疑い判定と分割再試行 | cpu | cpu | 負荷 | 1 | 115 | 1 | 1 | 判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| 冒頭欠落の疑い判定と分割再試行 | cpu | cpu | 負荷 | 2 | 115 | 1 | 1 | 判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し |
| ITN（漢数字 → 算用数字） | cuda | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | Rust の ITN がまだ無く、測れない |
| ITN（漢数字 → 算用数字） | cuda | - | 負荷 | 0 | 0 | 未測定 | 未測定 | Rust の ITN がまだ無く、測れない |
| ITN（漢数字 → 算用数字） | cpu | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | Rust の ITN がまだ無く、測れない |
| ITN（漢数字 → 算用数字） | cpu | - | 負荷 | 0 | 0 | 未測定 | 未測定 | Rust の ITN がまだ無く、測れない |
| 句読点（BERT、`ort` の CUDA） | cuda | fp32 | 無負荷 | 1 | 115 | 8303 | 10922 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cuda | fp32 | 無負荷 | 2 | 115 | 8688 | 11419 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cuda | fp32 | 無負荷 | 3 | 115 | 7825 | 11085 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cuda | fp32 | 負荷 | 1 | 115 | 6827 | 9840 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cuda | fp32 | 負荷 | 2 | 115 | 15398 | 37266 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cpu | fp32 | 無負荷 | 1 | 115 | 44378 | 51549 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cpu | fp32 | 無負荷 | 2 | 115 | 43828 | 53345 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cpu | fp32 | 無負荷 | 3 | 115 | 46491 | 57493 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cpu | fp32 | 負荷 | 1 | 115 | 43004 | 51345 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点（BERT、`ort` の CUDA） | cpu | fp32 | 負荷 | 2 | 115 | 40110 | 51270 | forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない |
| 句読点の後処理と疑問符の規則、置換辞書 | cuda | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | Rust の後処理がまだ無く、測れない |
| 句読点の後処理と疑問符の規則、置換辞書 | cuda | - | 負荷 | 0 | 0 | 未測定 | 未測定 | Rust の後処理がまだ無く、測れない |
| 句読点の後処理と疑問符の規則、置換辞書 | cpu | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | Rust の後処理がまだ無く、測れない |
| 句読点の後処理と疑問符の規則、置換辞書 | cpu | - | 負荷 | 0 | 0 | 未測定 | 未測定 | Rust の後処理がまだ無く、測れない |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 1 | 115 | 231596 | 268551 |  |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 2 | 115 | 241094 | 280550 |  |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 3 | 115 | 207832 | 258014 |  |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cuda | fp32 | 負荷 | 1 | 115 | 269037 | 305378 |  |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cuda | fp32 | 負荷 | 2 | 115 | 267191 | 306668 |  |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cpu | fp32 | 無負荷 | 1 | 115 | 67484 | 106500 |  |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cpu | fp32 | 無負荷 | 2 | 115 | 74335 | 117084 |  |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cpu | fp32 | 負荷 | 1 | 115 | 80814 | 129349 |  |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | cpu | fp32 | 負荷 | 2 | 115 | 84608 | 130848 |  |
| 話者の速報の割り当て（cosine と重心の更新） | cuda | transfer-floor | 無負荷 | 1 | 115 | 251 | 335 | 下限: 転送の往復だけで kernel を含まない。重心 8 個との cosine と更新 1 回 |
| 話者の速報の割り当て（cosine と重心の更新） | cuda | transfer-floor | 無負荷 | 2 | 115 | 246 | 373 | 下限: 転送の往復だけで kernel を含まない。重心 8 個との cosine と更新 1 回 |
| 話者の速報の割り当て（cosine と重心の更新） | cuda | transfer-floor | 無負荷 | 3 | 115 | 237 | 299 | 下限: 転送の往復だけで kernel を含まない。重心 8 個との cosine と更新 1 回 |
| 話者の速報の割り当て（cosine と重心の更新） | cuda | transfer-floor | 負荷 | 1 | 115 | 856 | 2065 | 下限: 転送の往復だけで kernel を含まない。重心 8 個との cosine と更新 1 回 |
| 話者の速報の割り当て（cosine と重心の更新） | cuda | transfer-floor | 負荷 | 2 | 115 | 610 | 2060 | 下限: 転送の往復だけで kernel を含まない。重心 8 個との cosine と更新 1 回 |
| 話者の速報の割り当て（cosine と重心の更新） | cpu | cpu | 無負荷 | 1 | 115 | 3 | 3 | 重心 8 個との cosine と更新 1 回 |
| 話者の速報の割り当て（cosine と重心の更新） | cpu | cpu | 無負荷 | 2 | 115 | 2 | 2 | 重心 8 個との cosine と更新 1 回 |
| 話者の速報の割り当て（cosine と重心の更新） | cpu | cpu | 負荷 | 1 | 115 | 3 | 3 | 重心 8 個との cosine と更新 1 回 |
| 話者の速報の割り当て（cosine と重心の更新） | cpu | cpu | 負荷 | 2 | 115 | 3 | 3 | 重心 8 個との cosine と更新 1 回 |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 1 | 115 | 129804 | 152495 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 2 | 115 | 127483 | 159378 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cuda | fp32 | 無負荷 | 3 | 115 | 97115 | 132238 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cuda | fp32 | 負荷 | 1 | 115 | 201700 | 260931 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cuda | fp32 | 負荷 | 2 | 115 | 208285 | 283090 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cpu | fp32 | 無負荷 | 1 | 115 | 33588 | 39980 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cpu | int8 | 無負荷 | 1 | 115 | 23752 | 30801 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cpu | fp32 | 無負荷 | 2 | 115 | 39077 | 42954 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cpu | int8 | 無負荷 | 2 | 115 | 29120 | 33888 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cpu | fp32 | 負荷 | 1 | 115 | 41394 | 49325 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cpu | int8 | 負荷 | 1 | 115 | 32040 | 36624 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cpu | fp32 | 負荷 | 2 | 115 | 42048 | 47455 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | cpu | int8 | 負荷 | 2 | 115 | 30065 | 37595 | 分割、埋め込み、clustering を含む 1 呼び出し |
| 清書の clustering（FastClustering）と全体の対応づけ | cuda | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | sherpa-onnx の中で再分離と同じ呼び出しに含まれ、分けて測る口が無い |
| 清書の clustering（FastClustering）と全体の対応づけ | cuda | - | 負荷 | 0 | 0 | 未測定 | 未測定 | sherpa-onnx の中で再分離と同じ呼び出しに含まれ、分けて測る口が無い |
| 清書の clustering（FastClustering）と全体の対応づけ | cpu | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | sherpa-onnx の中で再分離と同じ呼び出しに含まれ、分けて測る口が無い |
| 清書の clustering（FastClustering）と全体の対応づけ | cpu | - | 負荷 | 0 | 0 | 未測定 | 未測定 | sherpa-onnx の中で再分離と同じ呼び出しに含まれ、分けて測る口が無い |
| 訳 T と E（llama-server、CUDA） | cuda | CAT-Translate-3.3b-Q8_0 | 無負荷 | 1 | 115 | 256584 | 409758 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 訳 T と E（llama-server、CUDA） | cuda | CAT-Translate-3.3b-Q8_0 | 無負荷 | 2 | 115 | 250591 | 400465 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 訳 T と E（llama-server、CUDA） | cuda | CAT-Translate-3.3b-Q8_0 | 無負荷 | 3 | 115 | 252986 | 422328 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 訳 T と E（llama-server、CUDA） | cuda | CAT-Translate-3.3b-Q8_0 | 負荷 | 1 | 115 | 429943 | 739136 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 訳 T と E（llama-server、CUDA） | cuda | CAT-Translate-3.3b-Q8_0 | 負荷 | 2 | 115 | 439928 | 749299 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 訳 T と E（llama-server、CUDA） | cpu | CAT-Translate-3.3b-Q8_0 | 無負荷 | 1 | 115 | 2828573 | 4849719 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 訳 T と E（llama-server、CUDA） | cpu | CAT-Translate-3.3b-Q8_0 | 無負荷 | 2 | 115 | 2887817 | 4697008 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 訳 T と E（llama-server、CUDA） | cpu | CAT-Translate-3.3b-Q8_0 | 負荷 | 1 | 115 | 3330031 | 5484474 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 訳 T と E（llama-server、CUDA） | cpu | CAT-Translate-3.3b-Q8_0 | 負荷 | 2 | 115 | 3344179 | 5364090 | T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 認識の文字列 → 訳の入力 | cuda | transfer-floor | 無負荷 | 1 | 115 | 232 | 297 | 下限: 転送の往復だけで kernel を含まない。依頼の本文（JSON）を作るまで |
| 認識の文字列 → 訳の入力 | cuda | transfer-floor | 無負荷 | 2 | 115 | 225 | 281 | 下限: 転送の往復だけで kernel を含まない。依頼の本文（JSON）を作るまで |
| 認識の文字列 → 訳の入力 | cuda | transfer-floor | 無負荷 | 3 | 115 | 219 | 301 | 下限: 転送の往復だけで kernel を含まない。依頼の本文（JSON）を作るまで |
| 認識の文字列 → 訳の入力 | cuda | transfer-floor | 負荷 | 1 | 115 | 498 | 927 | 下限: 転送の往復だけで kernel を含まない。依頼の本文（JSON）を作るまで |
| 認識の文字列 → 訳の入力 | cuda | transfer-floor | 負荷 | 2 | 115 | 541 | 1041 | 下限: 転送の往復だけで kernel を含まない。依頼の本文（JSON）を作るまで |
| 認識の文字列 → 訳の入力 | cpu | cpu | 無負荷 | 1 | 115 | 2 | 3 | 依頼の本文（JSON）を作るまで |
| 認識の文字列 → 訳の入力 | cpu | cpu | 無負荷 | 2 | 115 | 2 | 2 | 依頼の本文（JSON）を作るまで |
| 認識の文字列 → 訳の入力 | cpu | cpu | 負荷 | 1 | 115 | 2 | 3 | 依頼の本文（JSON）を作るまで |
| 認識の文字列 → 訳の入力 | cpu | cpu | 負荷 | 2 | 115 | 2 | 2 | 依頼の本文（JSON）を作るまで |
| 照合 V と単位の換算 | cuda | transfer-floor | 無負荷 | 1 | 115 | 244 | 350 | 下限: 転送の往復だけで kernel を含まない。T の実際の出力に V と換算を掛けた 1 回 |
| 照合 V と単位の換算 | cuda | transfer-floor | 無負荷 | 2 | 115 | 251 | 311 | 下限: 転送の往復だけで kernel を含まない。T の実際の出力に V と換算を掛けた 1 回 |
| 照合 V と単位の換算 | cuda | transfer-floor | 無負荷 | 3 | 115 | 220 | 300 | 下限: 転送の往復だけで kernel を含まない。T の実際の出力に V と換算を掛けた 1 回 |
| 照合 V と単位の換算 | cuda | transfer-floor | 負荷 | 1 | 115 | 472 | 867 | 下限: 転送の往復だけで kernel を含まない。T の実際の出力に V と換算を掛けた 1 回 |
| 照合 V と単位の換算 | cuda | transfer-floor | 負荷 | 2 | 115 | 429 | 1776 | 下限: 転送の往復だけで kernel を含まない。T の実際の出力に V と換算を掛けた 1 回 |
| 照合 V と単位の換算 | cpu | cpu | 無負荷 | 1 | 115 | 8 | 13 | T の実際の出力に V と換算を掛けた 1 回 |
| 照合 V と単位の換算 | cpu | cpu | 無負荷 | 2 | 115 | 8 | 12 | T の実際の出力に V と換算を掛けた 1 回 |
| 照合 V と単位の換算 | cpu | cpu | 負荷 | 1 | 115 | 9 | 15 | T の実際の出力に V と換算を掛けた 1 回 |
| 照合 V と単位の換算 | cpu | cpu | 負荷 | 2 | 115 | 9 | 14 | T の実際の出力に V と換算を掛けた 1 回 |
| 字幕の配信（SSE）と描画 | cuda | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | Rust の字幕 server がまだ無く、測れない |
| 字幕の配信（SSE）と描画 | cuda | - | 負荷 | 0 | 0 | 未測定 | 未測定 | Rust の字幕 server がまだ無く、測れない |
| 字幕の配信（SSE）と描画 | cpu | - | 無負荷 | 0 | 0 | 未測定 | 未測定 | Rust の字幕 server がまだ無く、測れない |
| 字幕の配信（SSE）と描画 | cpu | - | 負荷 | 0 | 0 | 未測定 | 未測定 | Rust の字幕 server がまだ無く、測れない |

## 転送

`nsys profile` の capture range を認識の 1 呼び出し（decode）ごとに開き、1 発話あたりの値の p50 と p95 を取った。

| 向き | variant | 回数 p50 | 回数 p95 | bytes p50 | bytes p95 | 時間_us p50 | 時間_us p95 |
|---|---|---|---|---|---|---|---|
| host→device | fp32 | 450 | 645 | 4367000 | 5795000 | 1524 | 1812 |
| device→host | fp32 | 298 | 428 | 4894000 | 8304000 | 935 | 1670 |
| host→device | fp16 | 450 | 645 | 3216000 | 4652000 | 849 | 1075 |
| device→host | fp16 | 298 | 428 | 4894000 | 8324000 | 805 | 1203 |
| host→device | int8 | 1592 | 2047 | 59141000 | 84014000 | 19976 | 27319 |
| device→host | int8 | 856 | 1116 | 50404000 | 72476000 | 16515 | 23707 |

## JIT の cache と decode_ms

cache が空の process と、同じ cache で続けた process の、認識の最初の 1 回（fp32、create の後）と p50。

| cache | run | decode_ms 最初の1回 | decode_us p50 |
|---|---|---|---|
| empty | 1 | 529 | 175021 |
| warm | 1 | 358 | 169531 |
| warm | 2 | 369 | 175123 |
| warm | 3 | 381 | 169589 |

## node の置き場（ort）

`ort` が session を作るときに ONNX Runtime が出す "Node(s) placed on" の行を、provider ごとに足した数。device は session を作った provider。

| model | device | CUDA の node | CPU の node |
|---|---|---|---|
| encoder-epoch-35-avg-1.onnx | cuda | 3537 | 1599 |
| encoder-epoch-35-avg-1.fp16.onnx | cuda | 3564 | 1611 |
| punct_bert.onnx | cuda | 453 | 77 |

## 置き場の提案

| 段 | 提案 | 根拠 |
|---|---|---|
| 音声の取り込み（WASAPI のマイク、loopback） | 未決 | Rust の取り込み（WASAPI）がまだ無く、測れない |
| int16 → float32、16 kHz への resample、mix | 未決 | CPU の p50 が GPU の転送の下限以上で、GPU の kernel の時間が要る。無負荷の p50: GPU 345 µs（transfer-floor） / CPU 2455 µs（cpu）。負荷下の p50: GPU 368 µs（transfer-floor） / CPU 3195 µs（cpu） |
| 音声の ring buffer と preroll（1.0 秒） | CPU | CPU の p50 が GPU の転送だけの下限より小さい。無負荷の p50: GPU 258 µs（transfer-floor） / CPU 16 µs（cpu）。負荷下の p50: GPU 661 µs（transfer-floor） / CPU 20 µs（cpu） |
| VAD（Silero、sherpa-onnx の `provider`） | CPU | 無負荷の p50: GPU 388495 µs（fp32） / CPU 39519 µs（fp32）。負荷下の p50: GPU 1120604 µs（fp32） / CPU 42044 µs（fp32） |
| 言語判定（whisper-tiny） | GPU | 無負荷の p50: GPU 75549 µs（fp32） / CPU 123978 µs（int8）。負荷下の p50: GPU 93586 µs（fp32） / CPU 132489 µs（int8） |
| 認識の encoder（全 model） | 未決（評議） | sherpa-onnx の認識全体 fp32、無負荷の p50: GPU 171414 µs / CPU 151299 µs。sherpa-onnx の認識全体 fp32、負荷の p50: GPU 375176 µs / CPU 175025 µs。ort で encoder だけ fp32、無負荷の p50: GPU 66262 µs / CPU 121091 µs。ort で encoder だけ fp32、負荷の p50: GPU 92936 µs / CPU 117756 µs。参考（同じ精度の対ではない）: CPU の int8 は 128615 µs（int8 は CUDA に無い）、GPU の fp16 は 459578 µs（fp32 の GPU は 171414 µs） |
| fbank と探索（sherpa-onnx の中） | CPU（sherpa-onnx の中。provider に依らず host で走る） | 無負荷の p50: GPU 5572 µs（fp16） / CPU 7475 µs（fp32）。負荷下の p50: GPU 6356 µs（fp32） / CPU 8387 µs（int8） |
| 冒頭欠落の疑い判定と分割再試行 | CPU | CPU の p50 が GPU の転送だけの下限より小さい。無負荷の p50: GPU 232 µs（transfer-floor） / CPU 1 µs（cpu）。負荷下の p50: GPU 695 µs（transfer-floor） / CPU 1 µs（cpu） |
| ITN（漢数字 → 算用数字） | 未決 | Rust の ITN がまだ無く、測れない |
| 句読点（BERT、`ort` の CUDA） | GPU | 無負荷の p50: GPU 8272 µs（fp32） / CPU 44899 µs（fp32）。負荷下の p50: GPU 11112 µs（fp32） / CPU 41557 µs（fp32） |
| 句読点の後処理と疑問符の規則、置換辞書 | 未決 | Rust の後処理がまだ無く、測れない |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | CPU | 無負荷の p50: GPU 226840 µs（fp32） / CPU 70909 µs（fp32）。負荷下の p50: GPU 268114 µs（fp32） / CPU 82711 µs（fp32） |
| 話者の速報の割り当て（cosine と重心の更新） | CPU | CPU の p50 が GPU の転送だけの下限より小さい。無負荷の p50: GPU 244 µs（transfer-floor） / CPU 2 µs（cpu）。負荷下の p50: GPU 733 µs（transfer-floor） / CPU 3 µs（cpu） |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | CPU | 無負荷の p50: GPU 118134 µs（fp32） / CPU 26436 µs（int8）。負荷下の p50: GPU 204992 µs（fp32） / CPU 31052 µs（int8） |
| 清書の clustering（FastClustering）と全体の対応づけ | 未決 | sherpa-onnx の中で再分離と同じ呼び出しに含まれ、分けて測る口が無い |
| 訳 T と E（llama-server、CUDA） | GPU | 無負荷の p50: GPU 253387 µs（CAT-Translate-3.3b-Q8_0） / CPU 2858195 µs（CAT-Translate-3.3b-Q8_0）。負荷下の p50: GPU 434935 µs（CAT-Translate-3.3b-Q8_0） / CPU 3337105 µs（CAT-Translate-3.3b-Q8_0）。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない |
| 認識の文字列 → 訳の入力 | CPU | CPU の p50 が GPU の転送だけの下限より小さい。無負荷の p50: GPU 225 µs（transfer-floor） / CPU 2 µs（cpu）。負荷下の p50: GPU 519 µs（transfer-floor） / CPU 2 µs（cpu） |
| 照合 V と単位の換算 | CPU | CPU の p50 が GPU の転送だけの下限より小さい。無負荷の p50: GPU 238 µs（transfer-floor） / CPU 8 µs（cpu）。負荷下の p50: GPU 450 µs（transfer-floor） / CPU 9 µs（cpu） |
| 字幕の配信（SSE）と描画 | 未決 | Rust の字幕 server がまだ無く、測れない |

## H25 言語判定のやり直し

先頭 4 秒を渡す言語判定を、4 秒以上の発話で確定のたびにやり直したときの 1 回の時間（µs）。標本は 4 秒以上の発話だけ。

| provider | variant | 負荷 | 標本数 | p50_us | p95_us |
|---|---|---|---|---|---|
| cuda | fp32 | 無負荷 | 255 | 78641 | 102797 |
| cuda | fp32 | 負荷 | 170 | 95440 | 121329 |
| cpu | fp32 | 無負荷 | 170 | 144597 | 169124 |
| cpu | int8 | 無負荷 | 170 | 127064 | 145311 |
| cpu | fp32 | 負荷 | 170 | 164705 | 186930 |
| cpu | int8 | 負荷 | 170 | 136384 | 156902 |

## 遅れ L の見積り

Rust の実行系（取り込みから字幕まで）がまだ無いので、`hayamimi-eval stream` で `L` は測れない。
次の表は、測れた段の和である。単位は ms。Python の行は `lat-ja` を `--min-silence` 350 ms で流した記録で、有効な標本だけ、`L` は 19 発話（発話 5、10、18、19 を除く）、認識は 20 発話（発話 10、19 を除く）。
「L の見積り」の足し算は、VAD の区切りと配信の遅れを Python と同じと仮定した値（ASSUMPTION）で、句読点、ITN、字幕の配信を含まない。
「Rust 認識 X / 訳 Y」は、認識を X の run、訳 T を Y の run から取った。llama-server は本番でも GPU に置くので、認識だけを CPU へ置く形は「認識 cpu / 訳 cuda」の行である。

| 実装 | 指標 | 標本数 | p50 | p95 |
|---|---|---|---|---|
| Python（記録） | final の latency_ms（L_dec） | 199 | 311 | 446 |
| Python（記録） | L（発話の終わりから訳） | 189 | 1527 | 2061 |
| Rust 認識 cuda / 訳 cuda 無負荷 | 認識（create_stream から get_result） | 315 | 184 | 254 |
| Rust 認識 cuda / 訳 cuda 無負荷 | L の見積り = Python の認識以外の遅れの p50 902 ms + 認識 + 訳 T | 285 | 1349 | 1573 |
| Rust 認識 cuda / 訳 cuda 負荷 | 認識（create_stream から get_result） | 210 | 402 | 564 |
| Rust 認識 cuda / 訳 cuda 負荷 | L の見積り = Python の認識以外の遅れの p50 902 ms + 認識 + 訳 T | 190 | 1712 | 2180 |
| Rust 認識 cpu / 訳 cpu 無負荷 | 認識（create_stream から get_result） | 210 | 140 | 212 |
| Rust 認識 cpu / 訳 cpu 無負荷 | L の見積り = Python の認識以外の遅れの p50 902 ms + 認識 + 訳 T | 190 | 4053 | 6194 |
| Rust 認識 cpu / 訳 cpu 負荷 | 認識（create_stream から get_result） | 210 | 165 | 245 |
| Rust 認識 cpu / 訳 cpu 負荷 | L の見積り = Python の認識以外の遅れの p50 902 ms + 認識 + 訳 T | 190 | 4502 | 6785 |
| Rust 認識 cpu / 訳 cuda 無負荷 | L の見積り = Python の認識以外の遅れの p50 902 ms + 認識 + 訳 T | 190 | 1310 | 1543 |
| Rust 認識 cpu / 訳 cuda 負荷 | L の見積り = Python の認識以外の遅れの p50 902 ms + 認識 + 訳 T | 190 | 1511 | 1890 |

## 未検証

- 訳 T は WSL の llama-server（CUDA 12.0 の build）で測った。設計の 3.12 は CUDA 13.4 だけを指していて食い違う。Windows の native、CUDA 13 の llama-server で測り直すまで、訳 T の行は本番の値ではない。
- 負荷の列は、WSL の llama-server と Windows の ONNX Runtime が GPU を分けた値で、本番の分け方ではない（NOT VERIFIED）。
- encoder だけの行の入力は、発話から作った log-mel で、認識器の front end とは値が同じではない。句読点の BERT の行の入力は、punct_ja_parity.json の最初の 23 case で、lat-ja の発話の文字列ではない。どちらも時間は入力の値ではなく長さに従うと見て使ったが、確かめていない。
- CUDA で CPU に置かれた encoder の node は、fp32 が 1599、fp16 が 1611。差が 1 割に満たないので、CPU に置かれた node の数では fp16 の遅さを説明できない。原因は未特定である。
