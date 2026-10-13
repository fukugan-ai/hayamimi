# Python の基準（R0）

今の Python（`scripts/realtime_transcribe.py`）の速さと誤り率を、同じ機械で測って固定した記録である。
Rust 版の結果と比べる相手になる。

## 結論

発話の終わりから訳の字幕までの遅れ `L` は、`--min-silence` 0.35 の 2 回で p50 が 1527 ms と 1519 ms、p95 が 2061 ms と 2069 ms だった。
発話の終わりから確定文までの遅れ `L_asr` は、p50 が 1185 ms と 1132 ms、p95 が 1779 ms と 1801 ms だった。
2 回の `L` の p50 は 8 ms しか違わない。

標本は 1 回あたり 115（23 発話 × 5 回）で、有効、final なし、終端不正の 3 つに分かれる。
1 回目は 100 + 12 + 3、2 回目は 99 + 13 + 3 だった。
`L` の比べる発話は 19、`L_asr` の比べる発話は 20 である。

`--min-silence` は 0.30 を選んだ。
ja の CER は 0.30 が 0.1295 で、0.35 の 0.1325 を超えない。
`L_asr` の p50 は 0.30 が 1104 ms で、0.35 の run 1 より短い。
ただし 0.35 の 2 回目の CER は 0.1414 で、1 回目と 0.0089 離れる。
この揺れは 0.25 から 0.35 の間の差より大きい。
CER だけで 0.30 が最良と言い切れるわけではない。

DER は `--method refine_diarize` で 3 回測り、平均が 13.3%、13.1%、13.1% だった。
3 回の平均は 13.17%、幅は 0.2 pt である。

訳の chrF 4 言語（RB-7）は記録と 0.01 以内で合った。
全 `final` の `latency_ms` の平均は、WebSocket で受けた event と Python の標準出力の 2 つの経路で、本数が同じ 123 本、差が 0.0060 ms 以内で一致した（RB-5）。

次の 3 つの記録は、測った条件（command、入力の音声、版）が残っていない。
受入の期待値にも陽性対照にも使わず、「記録（条件が残っていない物）」の表に並べた。

- `final` の `latency_ms` の平均 98 から 103 ms
- 90 秒の 5 言語の確定の平均 236 ms と最大 552 ms
- 平均 DER 13.9%

測った機械：Windows 11 Pro 10.0.26300、AMD Ryzen 7 5700X（8 コア）、RAM 63.9 GB、NVIDIA GeForce RTX 5060 Ti（16311 MiB、driver 610.88）
Python の版：3.14.7
package の版：sacrebleu 2.6.0、jiwer 4.0.0、simpleder 0.0.5、OpenCC 1.4.2、pyarrow 25.0.1、sherpa-onnx 1.13.6、ctranslate2 4.8.1、onnxruntime 1.29.0、numpy 2.4.6、fsspec 2026.9.0、scipy 1.18.1、aiohttp 3.14.5、requests 2.34.2
道具：rustc 1.99.0、repo の HEAD は ec0f13e（未 commit の変更つき）、target は x86_64-pc-windows-msvc
日時：2026-10-13

## 記録（条件が残っていない物）

次の記録は、今回の測定と並べるだけで、合否の基準にしない。

| 項目 | 記録 | 今回の測定 |
|---|---|---|
| `final` の `latency_ms` の平均 | 98 から 103 ms（ja の確定、句読点込み） | 全 `final` 123 本で 266.80 ms（run 1）と 267.36 ms（run 2）。発話の最後の `final` だけ（103 本と 102 本）では 286.9 ms と 286.6 ms |
| 90 秒の 5 言語の確定の平均と最大 | 236 ms と 552 ms | 測っていない |
| 平均 DER | 13.9% | 13.17%（3 回の平均、幅 0.2 pt） |
| 既定の method の平均 DER | 25.7% | 25.5%（1 回、`der_eval_default_method.log`） |

`final` の平均が 98 から 103 ms と 287 ms で違って見えるのは、母集団が違うからである。
発話の最後の `final` だけの平均には、長い発話の `final` が偏って入る。
全 `final` の平均で比べるときは、本数を同じにして比べる（RB-5）。

## RB-1 採点と評価の package

`pip show` の 5 つの `Version:` は 2.6.0、4.0.0、0.0.5、1.4.2、25.0.1 で、陽性対照の sherpa-onnx は 1.13.6 だった（`testdata/baseline/pip_show.txt`）。
入れる前は 5 つとも `Package(s) not found` だった（`pip_show_before_install.txt`）。

`requirements-dev.txt` に無い 3 つを足した。
scipy は simpleder が import する。
aiohttp と requests は `eval_translate.py` が HTTP で parquet を読むときに要る。
足す前の run は import の error で終わった（`pip_install_extra.log`）。

## RB-2 評価の音声と manifest

出所の行の番号が無い組は、eval_real、eval_real_zhko、eval_real_yue、eval_diar、lat-ja、lat-en の 6 組である。
どの組も、wav の sha256 で照らす。

| 組 | 行の数 |
|---|---|
| eval_real | 30 |
| eval_real_zhko | 24 |
| eval_real_yue | 12 |
| eval_diar | 5 |
| lat-ja | 23 発話 |
| lat-en | 15 発話 |

2 回目の `--pin` の出力は、eval_real、eval_real_zhko、eval_real_yue、eval_diar の 4 組とも 1 回目と同じ bytes だった（`pin_second/`）。

発話の終わり（`end_sample`）は、`fetch-set --concat` が clip の最後の sample でなく、声の終わりに置く。
golden の clip は声の後ろに 1 から 2 秒の部屋の音が付いている。
Python の VAD は声の終わりから 0.35 秒で区切るので、clip の最後に置くと `final` が `t_end` より前に着く。
その置き方では 115 標本のうち 55 しか `final` が取れなかった（`superseded_end_at_clip_end/`）。
声の終わりは 20 ms の frame の音量で決める。
結合した wav の bytes は、この変更の前後で変わらない。

声の後ろに BGM が続く clip（発話 18 と 19）は、音量で声と分けられない。
この 2 発話の標本は、発話 18 が終端不正か final なしに、発話 19 が final なしに入る（RB-5）。

## RB-3 scorecard の取り直し

`scripts/eval_engine.py` を 2 回打った。
各回の前に `docs/results/scorecard.md` を `testdata/baseline/scorecard_record.md` へ退避し、各回の後に出力を `scorecard_run<n>.md` へ写して書き戻した。
書き戻した後の `docs/results/scorecard.md` は `scorecard_record.md` と bytes が同じだった（`cmp` が rc=0）。

| lang | 記録 | run 1 | run 2 | 2 回の差 |
|---|---|---|---|---|
| ja | 0.038 | 0.038 | 0.038 | 0.000 |
| en | 0.023 | 0.023 | 0.023 | 0.000 |
| zh | 0.066 | 0.066 | 0.066 | 0.000 |
| ko | 0.081 | 0.081 | 0.081 | 0.000 |
| yue | 記録に行なし | 0.061 | 0.061 | 0.000 |

mean RTF は記録の ja 0.090 より遅く、run 1 が 0.129、run 2 が 0.148 だった。
誤り率の判定には使わない。

## RB-4 golden 8 本

`scripts/make_ja_golden.py --check` は `0 of 8 clips differ` で、終了 code は 0 だった（`golden_check.log`、`golden_check.rc`）。
この check の前に `scripts/make_fleursset.py` で `testdata/fleurs_bench/ja/` を作る必要があった（`rb04_make_fleursset_ja.log`）。

## RB-5 遅れ `L` と `L_asr`（`--min-silence` 0.35）

lat-ja の 23 発話を 5 回ずつ流した run を 2 回取った。
単位は ms である。

標本の枠 115（23 発話 × 5 回）は、次の 3 つのどれか 1 つに入る。

- **有効**：`final` が在り、`L_asr` から Python の `final` の `latency_ms`（`L_dec`）を引いた値が `--min-silence` の 350 ms 以上の標本
- **final なし**：その標本の区間に `final` が 1 件も無い標本
- **終端不正**：`final` は在るが、`L_asr` から `L_dec` を引いた値が 350 ms 未満の標本。`t_end` が声の終わりより後ろに置かれている

| 回 | 有効 | final なし | 終端不正 | 和 |
|---|---|---|---|---|
| 1 | 100 | 12 | 3 | 115 |
| 2 | 99 | 13 | 3 | 115 |

`L_asr` の p50 と p95 は有効な標本で取り、`L` と `L_tr` は有効な標本のうち訳が取れた標本で取った。

| run | L p50 | L p95 | L_asr p50 | L_asr p95 | L_tr p50 | L_tr p95 | L_tr 負 | 訳の取れない有効な標本 |
|---|---|---|---|---|---|---|---|---|
| 1 | 1527 | 2061 | 1185 | 1779 | 213 | 1066 | 0 | 5 |
| 2 | 1519 | 2069 | 1132 | 1801 | 223 | 1116 | 0 | 5 |

run 1 と run 2 の L の p50 の差：8 ms

`final` が出なかった標本は、両方の run で同じ発話に集まる。
発話 10 が 5 回（Python が `final` を 1 つも出さない）、発話 19 が 5 回、発話 18 が 2 回、run 2 だけ発話 21 が 1 回である。
終端不正の 3 標本は、両方の run で発話 18 の繰り返し 1、3、5 である。

発話ごとの比べる集合は、Python の全 run で、その発話の中央値が取れる発話である。
`L` の比べる集合は 19 発話で、発話 5、10、18、19 を除く。
発話 5 は有効な標本が在るが、訳が 1 回も取れない。
`L_asr` の比べる集合は 20 発話で、発話 10、18、19 を除く。

WebSocket で受けた全 `final` の `latency_ms` と、Python の標準出力の `latency=<値>ms` を、別々に集計した。
本数は同じで、平均の差は 0.1 ms 以内に収まった（Python の標準出力は整数 ms で出すので、丸めの誤差が平均に残る）。

| 回 | WebSocket の本数 | WebSocket の平均 | 標準出力の本数 | 標準出力の平均 | 差 |
|---|---|---|---|---|---|
| 1 | 123 | 266.8027 | 123 | 266.7967 | 0.0060 |
| 2 | 123 | 267.3578 | 123 | 267.3577 | 0.0001 |

WebSocket は `jq -s -c '[.[] | select(.event.type == "final") | .event.latency_ms] | {n: length, mean: (add / length)}' lat_ja_0.35_<n>.events.jsonl`、標準出力は `grep -oE 'latency=[0-9.]+ms' server_0.35_<n>.out.log | sed -E 's/latency=([0-9.]+)ms/\1/' | awk '{s += $1; c++} END {print c, s / c}'` で集計した。
発話の最後の `final` だけ（標本のうち `final` が在る 103 本と 102 本）の平均は 286.9 ms と 286.6 ms で、母集団が違うので上の 2 つとは比べない。

`final` の言語判定は、Python の `asr_engine.py` が 4 秒未満の発話にだけ早めの判定を使い、それより長い発話では確定のたびにやり直す。
標準出力の `lid=<値>ms` では、やり直した `final` が両方の run で 74 本あり、その平均は run 1 が 114.1 ms、run 2 が 113.0 ms だった。
全 123 本の平均は 68.7 ms と 68.0 ms である。
Rust でこの規則を写すか変えるかは、R1 の言語判定の GPU と CPU の実測の後に、R3 が決める（設計の H25）。

run 1 の間は `cargo test`（2 job）と sacrebleu の CLI を並行して走らせた。
run 2 の間は jq の集計だけだった。

発話ごとの `L` の中央値（nearest-rank）を示す。
有効な標本のうち訳が取れた標本だけで取り、`-` は 5 回とも取れなかった発話である。

| utt | run 1 | run 2 |
|---|---|---|
| 1 | 1980 | 2069 |
| 2 | 1861 | 1918 |
| 3 | 1644 | 1570 |
| 4 | 1028 | 1126 |
| 5 | - | - |
| 6 | 1877 | 1776 |
| 7 | 2010 | 2053 |
| 8 | 1414 | 1387 |
| 9 | 1297 | 1270 |
| 10 | - | - |
| 11 | 1834 | 1773 |
| 12 | 1433 | 1388 |
| 13 | 1586 | 1640 |
| 14 | 1227 | 1175 |
| 15 | 1764 | 1813 |
| 16 | 1573 | 1588 |
| 17 | 1631 | 1633 |
| 18 | - | - |
| 19 | - | - |
| 20 | 1309 | 1262 |
| 21 | 1447 | 1410 |
| 22 | 1342 | 1303 |
| 23 | 1102 | 1184 |

## RB-6 `--min-silence` の掃引

各値を 1 回ずつ流した。
ja の CER は、23 発話 × 5 回の `final` の文字列と参照文の距離の合計を、参照文の長さの合計（2950）で割った値である。
`L_asr` は `final` が在る全標本で数えた。
0.35 の `L_asr` を RB-5 と同じ有効な標本だけで数えると、p50 が 1185、p95 が 1779 になる。
0.20 と 0.25 と 0.30 は、有効な標本だけで数えても同じ値である（終端不正が 0 本、0 本、1 本）。

| min-silence | L_asr p50 | L_asr p95 | ja CER |
|---|---|---|---|
| 0.20 | 1010 | 1542 | 0.1505 |
| 0.25 | 995 | 1635 | 0.1346 |
| 0.30 | 1104 | 1724 | 0.1295 |
| 0.35 | 1161 | 1779 | 0.1325 |

名指し：0.30

`L_asr` の p50 が最も短いのは 0.25 の 995 ms である。
0.25 の CER は 0.1346 で、0.30 より 0.0051 高い。
0.20 は CER が 0.1505 まで上がる。
0.35 の 2 回目の CER は 0.1414 だった。
各値は 1 回ずつなので、0.20 から 0.30 の揺れの幅は測っていない。
0.20 から 0.30 の run の間にも、`cargo clippy` や `cargo test`（2 job）を並行して走らせた。

## RB-7 訳の chrF と DER

chrF は `eval_translate.py`（n = 50、seed 0）で測り、記録と 0.01 以内で一致した。

| lang | backend | 記録 | 今回 |
|---|---|---|---|
| en | fugumt | 47.57 | 47.57 |
| zh | m2m | 20.19 | 20.19 |
| ko | m2m | 25.20 | 25.20 |
| es | m2m | 42.09 | 42.09 |

設計の command（`--targets en,zh,ko,es` を backend なしで 1 回）は既定の backend が M2M-100 で、en が 48.88 になる（`chrf_eval_all_default_backend.log`）。
本番の ja から en は FuguMT なので、`chrf_eval.log` は en を `--backend fugumt`、zh と ko と es を `--backend m2m` で測った 2 回の出力をつないだ物にした。

DER は `eval_diar.py --collar 0.25 --method refine_diarize` で、AMI の 5 会議を 3 回測った。
単位は % である。

| run | 平均 | ES2011a | IS1008a | ES2004a | IS1009a | TS3003a |
|---|---|---|---|---|---|---|
| 1（`der_eval.log`） | 13.3 | 17.5 | 4.1 | 17.3 | 13.7 | 13.9 |
| 2（`der_eval_run2.log`） | 13.1 | 16.8 | 4.1 | 17.3 | 13.7 | 13.7 |
| 3（`der_eval_run3.log`） | 13.1 | 16.8 | 4.1 | 17.3 | 13.7 | 13.7 |

DER 平均：13.17
DER 幅：0.2

平均は 3 つの run の平均 DER の算術平均を小数 2 桁に四捨五入した値、幅は最大から最小を引いた値である。
会議別では、run 1 の ES2011a（hyp_speakers 5）と TS3003a だけが run 2 と run 3 から離れる。
3 回では揺れの幅の見積もりが粗いので、Rust との差は幅 0.2 pt を目安にし、幅の中なら揺れの中と読む。

run 3 は他の測定と並べず 1 本だけ流した。
開始時は CPU 使用率 2%、GPU 使用率 0%、build と llama に当たる process が 3 件（名前は採っていない）で、終了時は CPU 使用率 8%、該当 process が 0 件だった（`der_run3.note`）。
run 1 と run 2 の間の負荷は記録していない。

設計の command（`--method` なし）は既定の method が `baseline` で、平均 25.5% になる（`der_eval_default_method.log`）。
この run は DER の run に数えない。
会議別の行に `diar_rtf=` が在るかで、`--method refine_diarize` の run を見分ける。

chrF の fixture は、`hayamimi-eval fleurs-pairs --degrade` の参照訳と、先頭の語（zh は先頭の 1 文字）を落とした hyp の対を、sacrebleu の CLI（`-m chrf -b -w 4`）で採点した物である（`chrf_fixture.tsv`、4 言語の文ごとの値と corpus の値で 204 行）。
corpus の値は en 96.6475、zh 97.8838、ko 94.2154、es 97.7614 だった。
参照訳をそのまま hyp にすると 4 言語とも 100.0000 になり、原文を hyp にすると en 0.5502、zh 4.9763、ko 0.3061、es 0.4012 になる（`chrf_control.tsv`）。

## RB-8 wire の録り

`hayamimi-eval record --wav tests/golden/ja/ja_014.wav` を、`--input ws --serve --translate en` で起こした Python に向けて録った（`testdata/baseline/wire/`）。
`translation` の event を録るため、設計の command に `--translate en` を足した。

録れた event の型は 7 種である。

- `ready`：1 件
- `partial`：22 件
- `final`：2 件
- `refine`：2 件
- `translation`：2 件
- `session_reset`：1 件
- `session_summary`：1 件

Python が出す型のうち、この録りに現れなかった物があるかは確かめていない。

`/` と `/dashboard` と `/transcript` と `/replacements` と `/itn_overrides` と `/config` の status は 200 で、応答の key は `wire/routes.json` に録った。
`/ingest` の handshake は status 101 を返し、応答の accept は録れた鍵から RFC 6455 の式で出した値と合った（`wire/ingest_handshake.json`）。

## 確かめていないこと

- 発話 18 と 19 の `t_end` は、声の後ろに BGM が続くために実際の声の終わりより遅い。全発話に当てる別の規則（Silero VAD の区切り、手で付けたラベル）で決め直すかは決めていない（設計の H26）。
- 発話 10 の clip で Python が `final` を出さない原因は調べていない。発話 10 の区間に Silero VAD だけを掛けて区切られた区間を数える測定は、まだ流していない。
- 言語判定をやり直さなかった `final` の `latency_ms` の平均を、`--translate` を付けない Python の run で取る測定は、まだ流していない。
- `e2f30ac` の版で IS1009a の DER を流す測定は、まだ流していない。
- 回の間の揺れは、`--min-silence` 0.35 の 2 回と DER の 3 回しか見ていない。
- RB-6 の `L_asr` は `final` が在る全標本で数えていて、RB-5 の有効な標本とは定義が違う。0.35 の行は、有効な標本だけで数えると p50 が 1185 になる。
