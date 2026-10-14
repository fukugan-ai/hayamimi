# hayamimi を Rust と GPU で置き換える

話してから訳した字幕が出るまでを、今の Python 版より短くする。
そのために、実行時の経路から Python を外して Rust にし、推論を GPU に載せる。
品質（認識の誤り率と訳の自然さ）は、速度の目標を満たした上で、今と同じか良くする。

この文書は日本語が正本である。
英語版 `rust_port.md` は 8 の slice R10 の成果物とする。
R0 の受入と、各受入 ID の下の case 行は `rust_port_tests.ja.md` に置く。

## 1. 結論

### 1.1 優先順位

速度が第一、品質が第二である。
速度と品質がぶつかる所では速度を取り、品質は速度の予算の中で一番良い形を選ぶ。

**主指標**：発話の終わりから、その発話の訳の字幕が出るまでの遅れ `L`。
p50 と p95 で測り、認識の遅れ `L_asr` と訳の遅れ `L_tr`（`L − L_asr`）を分けて記録する。
定義と測り方は 3.1 に置く。

### 1.2 作る物

- **`hayamimi`**（Rust の実行 file）：今の `scripts/realtime_transcribe.py` の CLI、音声の取り込み、認識、句読点、ITN、話者、清書、字幕の server を 1 つの process で持つ。
- **`hayamimi-asr`**（Rust の library）：認識の経路。
- **`hayamimi-gpu`**（Rust の library）：自分で build した CUDA 版の sherpa-onnx の C library と、その中の ONNX Runtime を開き、CUDA の device を数え、provider の割り当てを log に出す。
- **`hayamimi-ui`**（Dioxus web）：dashboard、OBS の overlay、transcript の頁。
- **`hayamimi-eval`**（Rust の開発用の実行 file）：段ごとの計測、採点、遅れの測り、評価用の音声の取得。実行時の経路には入らない。
- **`xtask`**（Rust の開発用の実行 file）：CUDA 版の sherpa-onnx の C library を、版を固定して build する（3.12）。
- **`hayamimi-relay`**（既存、slice 1 で作った物）：訳の中継。役割は会話字幕の設計のまま、訳の速度の節（3.6）を足す。

### 1.3 作らない物

- Python を残して Rust から呼ぶ経路。
- クラウドの認識と翻訳の API。
- CPU の翻訳器（FuguMT、M2M-100）の Rust 版。訳は言語モデル（llama-server）だけで行い、GPU が無ければ同じ model を CPU で動かす（3.6、3.11）。
- mobile/（Flutter）の作り直し。wire 形式の互換だけを守る（3.8）。
- 精度か速度を今より落とした状態で「置き換えた」とすること。

### 1.4 GPU の既定

全段を GPU に載せるのを既定とする。
CPU に残す段は、段ごとに「GPU の方が遅いという実測」か「GPU へ移して縮む時間が `L` に対して無視できるという実測」か「GPU で動く経路が無いという根拠」を 3.5 の表に持つ。
根拠の無い段を CPU に残さない。
GPU と CPU のどちらが速いかの実測は、R1（計測の slice）で GPU の経路が在る段について取る。
GPU の経路が無い 5 段（取り込み、ITN、句読点の後処理、清書の clustering、配信）は、その段を作る slice が CPU の時間を測る（3.5）。
GPU が無い機械と、段の CUDA の session が作れない機械では、GPU に置いた段をその段ごとに CPU へ落として動き続け、落ちたことを log の event と字幕の画面に出す（3.11）。

### 1.5 認識の engine

認識は、sherpa-onnx の C library を CUDA を有効にして自分で build し、Rust の `sherpa-onnx` crate（1.13.8）を `SHERPA_ONNX_LIB_DIR` でその build に繋ぐ。
話者の埋め込み（CAM++）、清書の再分離（pyannote）、言語判定（whisper-tiny）、VAD（Silero）も、同じ build の `provider` で動かす。
句読点の model（BERT）は sherpa-onnx の型に無いので、同じ build に入っている ONNX Runtime を `ort` crate の `load-dynamic` で開き、CUDA の実行 provider で動かす（3.4）。

## 2. 数えた結果

数えた日時は 2026-10-13、範囲は repo の git 管理下の全 file（`native/target`、`models/`、`testdata/` は git 管理外で数に入らない）である。

### 2.1 Python の在り処

| pattern（式） | 結果 | 陽性対照 |
|---|---|---|
| `git ls-files '*.py' \| wc -l` | 78 本 | `git ls-files \| grep -c '\.py$'` も 78 |
| `git ls-files '*.py' \| xargs wc -l` の total | 26,979 行 | 最大は `scripts/realtime_transcribe.py` の 2,635 行 |
| 置き場の内訳 | `scripts/` 56、`tests/` 20、`demo/` 2 | 合計 78 |

実行時に読まれる module は、`scripts/realtime_transcribe.py` から import を辿って決めた（`grep -E '^\s*(from\|import) '` を 18 本に掛けた）。
辿った先は `asr_engine`、`subtitle_server`、`speaker_id`、`diarize`、`global_recluster`、`punct_ja`、`itn_cjk`、`lid_preprocessing`、`audio_utils`、`ws_ingest`、`ws_protocol`、`translate_ja_en`、`translate_m2m` の 13 本である。
`lid_target_clf` と `ja_text_norm` は評価と学習の script だけが読む（`grep -lE "(from\|import) <名>\b"` で、前者は `eval_lid_voxlingua.py` と `tests/test_units.py`、後者は学習系 4 本）。

実行時の依存（`requirements.txt`）は sherpa-onnx 1.13.6、onnxruntime 1.29.0、ctranslate2 4.8.1、sentencepiece 0.2.2、fugashi 1.5.2、unidic-lite 1.0.8、soundcard 0.4.6、sounddevice 0.5.6、numpy、soundfile、psutil、huggingface_hub、kiwipiepy 0.23.2（任意、LGPL）である。
採点の依存（`requirements-dev.txt`）のうち、R0 が使う sacrebleu 2.6.0、jiwer 4.0.0、simpleder 0.0.5 は Windows の `.venv` に入っていない（評議の席の実測、2026-10-13）。

### 2.2 sherpa-onnx の使い方

`grep -ohE 'sherpa_onnx\.[A-Za-z_]+(\.[A-Za-z_]+)?'` を実行時の 4 本に掛けた結果、使っている型は 18 種である。
認識器は `OfflineRecognizer` の `from_transducer`（ReazonSpeech、Parakeet v2 と v3）、`from_paraformer`、`from_sense_voice`、`from_nemo_ctc`（parakeet-ja、second opinion）、`from_omnilingual_asr_ctc` で、ほかに `VoiceActivityDetector`（Silero）、`SpokenLanguageIdentification`（whisper-tiny）、`SpeakerEmbeddingExtractor`（CAM++）、`OfflineSpeakerDiarization`（pyannote segmentation-3.0 と `FastClustering`）を使う。
すべて非 streaming の認識器を VAD の区切りに掛ける形で、速報（partial）は伸びていく音声を約 0.5 秒ごとに解き直す。

Rust の公式 crate `sherpa-onnx` 1.13.8 の rustdoc に、上の型がすべて在り、`OfflineModelConfig`、`VadModelConfig`、`SpokenLanguageIdentificationConfig`、`SpeakerEmbeddingExtractorConfig`、`OfflineSpeakerSegmentationModelConfig` が `provider` を持つ（S1）。
crate の feature は `static` と `shared` だけで、build script は `SHERPA_ONNX_LIB_DIR` が在ればその directory の library に繋ぎ、無ければ CPU 版の archive を取る（S3）。
C の API は音声を `const float *samples` と個数で受ける（`SherpaOnnxAcceptWaveformOffline`、`SherpaOnnxVoiceActivityDetectorAcceptWaveform`。S11）。

### 2.3 手元の model

`models/`（3.1 GB）の 12 項目を `ls` で数えた。
実行時は int8 の file を読む（`asr_engine.py` の `RZ_MODEL_FILES` が `encoder-*.int8.onnx` ほか）。
fp32 と fp16 が手元に在るのは ReazonSpeech（3 種とも同じ tar に在る）、whisper-tiny（fp32）、pyannote（fp32）、句読点 BERT（fp32 だけ）、CAM++、Silero VAD である。
sherpa-onnx の `asr-models` の release（498 件）には、SenseVoice、Paraformer-zh、Omnilingual 300M の fp32 版と、Parakeet v2 の fp16 版が在る。
Parakeet v3 と parakeet-ja（tdt_ctc 0.6b ja）は int8 版しか見つからなかった（S2）。
訳の model の GGUF は WSL の `/mnt/h3-models`（`/dev/loop1` の ext4 の image。`findmnt` で確かめた）に在り、Windows からは直に読めない。

### 2.4 消える物と変わる物を読んでいる者

| 読む者 | 件数（式） | 扱い |
|---|---|---|
| md の中の `scripts/*.py` の名指し | 37 file、357 件（`git ls-files '*.md' \| xargs grep -oE 'scripts/[a-z_0-9]+\.py' \| wc -l`。陽性対照は README.md の `realtime_transcribe` 10 件） | 3.9 の表で仕分ける |
| md の中の `python ` の command | 68 件 | 同上 |
| md 以外（Dart、TS、Rust、yml、toml、json）の `scripts/*.py` | 52 件、38 file。大半は mobile の Dart が「どの Python から写したか」を書いた comment | 3.9 の表で仕分ける |
| 字幕の event の wire 形式 | event の型 13 種（`grep -ohE '"type": *"[a-z_]+"' scripts/*.py`）。HTTP の route は `/events`、`/dashboard`、`/transcript`、`/replacements`、`/itn_overrides`、`/config`、`/reset`、`/` の 8 本。`/ingest` の WebSocket | Rust でも同じ形を出す（RP-12）。読む者は mobile の `remote_event.dart` ほか、`demo/remotion` の `timeline.ts`、`docs/guide/embedding*.md`、訳の中継 |
| CI | `.github/workflows/test.yml` が `pip install` と `pytest tests` を打つ | R11 で cargo に替える |
| 会話字幕の設計 `conversation_subtitles.ja.md` | 認識を Python に残す決め、chrF を Rust で書かない決め、V を 1 回とする決め、llama-server を WSL に置く決め、gnu の target、`<relay>` の path、CS-3 と CS-20 | 書き替えた。slice 1 の case 行 T-CX-1-002 も、V が T の訳と E の訳に 1 回ずつの形に書き替えてある（会話字幕の設計 7.8） |
| `docs/design/goals.md` | 実行環境の行、GPU の名前、候補アプローチの見出し、残課題の GPU の行 | この文書と同じ手番で書き替えた |
| `README.md` の 5 行目、`README.ja.md` の 5 行目と 15 行目（CPU だけで動くという売り） | 3 行 | R10 で `readme-writing.md` の工程で書き替える |
| `state.json` の `constraints` の「Rust への移植は速度が足りないと測れたときだけ」 | 1 行 | この文書の目標と食い違う。state.json を書く席（セラ）が直す（10 の H22） |

### 2.5 今の速さの記録

`docs/results/benchmarks.md` と `goals.md` に在る確定の遅れは平均と最大だけで、p50 と p95 は無い（`grep -n -iE 'p95|p50|median'` が 0 件、同じ file の `平均` は複数件）。
記録は「ja 確定 平均 98〜103 ms（句読点込み）」「90 秒の 5 言語 平均 236 ms、最大 552 ms」で、この ms は VAD が区切りを決めてから確定文が出るまでである（`realtime_transcribe.py` の `drain_segments` が `vad.front` を取った時刻から測る）。
VAD が発話の終わりを決めるまでの無音待ち（`--min-silence`。この記録の時の値は 0.35 秒で、Rust の既定と比較の値は 0.30 秒。10 の H9）は含まない。
この 2 つの記録は、測った条件（command、入力の音声、版）が残っていない。
したがって受入の期待値にも陽性対照にも使わず、記録として並べるだけにする。
翻訳の遅れは記録が無い（Python の `translation` event は時刻も `id` も持たない）。
段ごとに GPU と CPU を比べた実測は、この repo にも、別の席の調べ（2026-10-13）にも無い。
したがって基準の値は R0 で、段ごとの比較は R1 で測る（3.1、8）。

## 3. 方針と理由

### 3.1 遅れの定義と測り方

発話ごとに次の時刻を取る。
4 つとも、実装の外の測りの道具（`hayamimi-eval stream`）の時計で取る。

| 記号 | 時刻 |
|---|---|
| `t_end` | 発話の最後の音声 sample を道具が送った時刻 |
| `t_final` | その発話の `final` event を道具が受けた時刻 |
| `t_tr` | その発話の訳の event のうち、訳の元にした原文の文字列がその発話の `final` の文字列と一致し、`status` が `ok` で `text` が空でない物を、道具が初めて受けた時刻 |
| `t_partial` | その発話の最初の `partial` event を道具が受けた時刻 |

`L = t_tr − t_end`、`L_asr = t_final − t_end`、`L_tr = t_tr − t_final` とする。
`t_tr` を「原文が `final` と一致した訳」に限るのは、速報の断片の訳で `L` が小さく、負にもなり、受入が中身の無いまま通るのを防ぐためである。
中継は訳の event に、訳の元にした原文の文字列の sha256 を `src_sha256` として載せ、道具は `final` の文字列の sha256 と比べる。
Python の `translation` event は `src_sha256` も `id` も持たないので、Python の run は「`final` の後、次の `final` までに最初に来た訳」で対応づけ、訳の取れない発話の数を別に記録する（`rust_port_tests.ja.md` の RB-5）。
`status` が `held` か `unavailable` の訳と `text` が空の訳は、字幕に何も出ないので `t_tr` に入れない。
Rust の run は、訳を `final` と同じ `id` で結び、`final` を発話へ `t1_s`（流れの先頭からの発話の終わり）で結ぶ。
着いた時刻で窓に割ると、遅れて着いた `final` が次の発話の窓に入り、その発話が「final なし」として母数から落ち、遅い標本ほど p95 から消えるからである。
Python の `final` は `t1_s` を持たないので着いた時刻で割り、前の窓が「final なし」で次の窓に `final` が 2 件以上在る標本の数を、遅れて着いた疑いとして別に数える（3.16）。
速報の訳（原文が `final` と一致しない訳）の時刻は `L` に入れず、RP-5 で別に測る。
速報の原文が最後の速報で `final` と同じ文字列になっていれば、その訳は `t_tr` に入り、`L_tr` が負になることがある。
その発話の数は結果の file に別に数える。

`/ingest` は今の Python も Rust も同じ protocol で持ち、同じ接続で event を返す（README の Network audio input）。
道具がこの口に音声を実時間の速さで送り、どの sample を何時に送ったかを覚える。
`t_end` は、評価の音声の manifest に書いた発話の終わりの sample を送った時刻とする。
同じ道具で Python と Rust を測るので、実装が自分の時計で自分を測ることにならない。

VAD が区切りを決めてから確定文が出るまでの時間（`L_dec`）は、外からは見えない。
`L_dec` は各実装が自分の log（Python は `final` の `latency_ms`、Rust は `asr.utterance.timed`）に出す値を記録だけにし、受入の合否に使わない。

評価の音声は 2 本を作る。

- **lat-ja**：`tests/golden/ja/` の 8 本と、ja の実放送の組（`testdata/eval_real` の ja 15 本）を、間に 1.5 秒の無音を挟んでつないだ物。23 発話。
- **lat-en**：en の実音声 15 本（同じ組の en）を同じ形でつないだ物。15 発話。

同じ音声を 5 回流す。
lat-ja の 1 回の run の標本の枠は 23 発話 × 5 回で、各標本は次の 3 つのどれか 1 つに入る。

```
枠（23 × 5） = 有効 + final なし + 終端不正
```

- **final なし**：その標本の区間に `final` が 1 件も無い。
- **終端不正**：`final` は在るが、`L_asr − L_dec < min_silence`（`min_silence` は run に渡した `--min-silence` の値）。
- **有効**：`final` が在り、`L_asr − L_dec ≥ min_silence`。

終端不正を分けるのは、VAD は声の終わりの後に `min_silence` の無音を数えてから区切るので、`t_end` が本当の声の終わりに在れば `L_asr` は `L_dec` より `min_silence` 以上長くなるからである。
この差が `min_silence` より短い標本は、`t_end` が声の終わりより後ろに置かれている（声の後ろに音楽が続く clip で起きる）。
その標本の `L_asr` は短く出るので、p50 と p95 にも対の比較にも入れない。
`L_asr` の p50 と p95 は有効な標本で取り、`L` と `L_tr` は有効な標本のうち訳が取れた物で取る。
3 つの数と、訳の取れない有効な標本の数は、結果の file に別々に書く。
`--min-silence` の掃引（`rust_port_tests.ja.md` の RB-6）の `L_asr` も、各値の run をその run に渡した `min_silence` で同じ式に掛けて仕分け、有効な標本で取る。
掃引で選んだ値は R1 以降の比較に使い、その比較は有効な標本で数えるので、掃引の表だけ終端不正を含めると、同じ設定の行が比較の時より短く出るからである（0.35 の 1 回目では、全標本の p50 が 1161 ms、有効な標本の p50 が 1185 ms）。

`t_end` を決める規則（manifest の `end_sample` の置き方）は lat-ja の全発話に同じ 1 つを当てる。
規則を変えるときは全発話の `end_sample` を同じ新しい規則で置き直し、Python の基準もその manifest で測り直す。
一部の発話の `end_sample` だけを結果を見て動かすと、測る物差しを結果に合わせて曲げることになるからである。
今の規則は 20 ms の frame の音量で声の終わりを決める（`baseline_python.md` の RB-2）。

標本は独立ではない。
23 種の発話の 5 回の繰り返しで、p95（nearest-rank、有効な標本の数を n として ceil(0.95 × n) 番目）は、遅い側の 1、2 種の発話の時間でほぼ決まる。
したがって p95 は「遅い発話でどれだけ待つか」として読み、偶然の揺れの幅としては読まない。
比べ方を 2 つ足す。

- **発話ごとの対の比較**：比較集合の発話のそれぞれで、5 回の中央値を Python と Rust で比べ、Rust が小さい発話の数を数える。**比較集合**は、Python の基準の全 run で、その発話の中央値が取れる発話とする（指標ごとに決める。`L` は訳の取れた有効な標本、`L_asr` は有効な標本で数える）。比較集合は Python の run だけで決め、Rust の結果では動かさない。
- **回の間の揺れ**：同じ実装の 5 回の間で、発話ごとの最大と最小の差を数える。

### 3.2 範囲：Python の file ごとの扱い

`移す` は Rust の置き場、`消す` は理由を書く。
「移さないで Python のまま残す」file は 0 本である（Python は規約で filmboard だけに使う）。
78 本 = 移す 30 本 + 消す 48 本（`scripts/` 27 本、`demo/` 1 本、`tests/` 20 本）。

**消すのは R11 だけ**で、R11 に入る条件は次の 3 つである。

1. R0 の基準の要約（遅れ、誤り率、chrF、DER の値と、測った条件）が、git 管理下の `docs/results/baseline_python.md` に在る。
2. RP-1、RP-13、RP-14、RP-15 が `<base>` の値と照らして通っている。RP-13 は 10 の H36 の扱いで、通るまで R11 は始めない。RP-1 の ko の R11 の入口の上限は、配る形の build の記録値 86939（百万分率）で、Rust の平均を丸めずに整数のまま比べ、超えたら評議へ出す（小数 3 桁へ丸めるのは Python の表と比べる RP-1 の本体だけ）。RP-1 の閾値の 0.081 は動かさず、ko の行は赤のまま 10 の H35 に残す。陽性対照は、Python の wheel の ORT を読ませた Rust の run の 66 clip が Python の log と全部一致し、ko が 0.081 になること（R3c の実測）である。ORT か sherpa-onnx の版を上げるたびに ko を測り直し、86939 と比べる。
3. RP-19 の対応表（Python の test の case → Rust の test の名前）が全行そろい、全部通っている。

消した file の中身は git の履歴に残り、R11 の commit の 1 つ前を指せば読める。

#### 移す（30 本）

| file | 移す先 | 理由 |
|---|---|---|
| `scripts/realtime_transcribe.py` | `hayamimi`（CLI、run_stream、Refiner、EventHub、中継への送り） | 実行時の本体 |
| `scripts/asr_engine.py` | `hayamimi-asr::route`、`::decode` | 言語の振り分けと認識 |
| `scripts/subtitle_server.py` | `hayamimi::server`（axum）と `hayamimi-ui` | 字幕の server と頁 |
| `scripts/speaker_id.py` | `hayamimi-asr::speaker` | 話者の速報 |
| `scripts/diarize.py` | `hayamimi-asr::diarize` | 清書の再分離 |
| `scripts/global_recluster.py` | `hayamimi-asr::recluster` | 実行時に option で読む |
| `scripts/punct_ja.py` | `hayamimi-asr::punct` | 句読点 |
| `scripts/itn_cjk.py` | `hayamimi-asr::itn` | 漢数字の変換 |
| `scripts/lid_preprocessing.py` | `hayamimi-asr::lid` | 言語判定の前処理 |
| `scripts/audio_utils.py` | `hayamimi-asr::audio` | 変換と resample |
| `scripts/ws_ingest.py`、`scripts/ws_protocol.py` | `hayamimi::ingest` | 網からの音声入力 |
| `scripts/download_models.py` | `hayamimi models fetch` | 入れる手順から Python を消す |
| `scripts/dump_ja_config.py` | `hayamimi config dump` | `docs/spec/ja_pipeline_spec.json` の出所 |
| `scripts/ws_mic_client.py` | `hayamimi-eval stream`（3.1 の測り） | 参照の client を測りに使う |
| `scripts/check_doc_links.py` | `hayamimi-eval doc-links` | 文書の link の検査 |
| `scripts/eval_accuracy.py`、`scripts/eval_engine.py`、`scripts/eval_common.py` | `hayamimi-eval scorecard` | RP-1 の採点 |
| `scripts/bench_offline.py` | `hayamimi-eval stages`（RP-23） | RTF と段ごとの時間 |
| `scripts/eval_fleurs_bench.py` | `hayamimi-eval fleurs` | FLEURS の 5 言語の採点 |
| `scripts/eval_translate.py` | `hayamimi-eval fleurs-pairs` と `hayamimi-eval chrf` | 訳の chrF（RP-13、RP-14） |
| `scripts/eval_diar.py` | `hayamimi-eval der` | DER（RP-15） |
| `scripts/make_ja_golden.py` | `hayamimi-eval golden` | RP-2 |
| `scripts/make_realset.py`、`make_realset_zhko.py`、`make_realset_yue.py`、`make_fleursset.py`、`make_diarset.py` | `hayamimi-eval fetch-set` | R0 で書いた manifest に固定した行だけを取り直す。行を選ぶ規則（頁送り、長さの絞り）は移さない |
| `demo/capture_events.py` | `hayamimi-eval record` | demo の event を録る |

評価の 5 本の取得を「manifest に固定した行だけ取り直す」にしたのは、行の選び方を移すと、選び方が同じであることの検査が 1 つ増えるからである。
manifest には、元の dataset の名前、split、行の番号、音声の sha256 を R0 で書く（sha256 は記録で、取得の可否の条件にしない）。

#### 消す（48 本、R11 で）

| file | 理由 |
|---|---|
| `scripts/translate_ja_en.py`、`scripts/translate_m2m.py` | 訳は GPU の言語モデルに替える（3.6）。品質は RP-13 で今以上を測る |
| `scripts/translate_candidates.py`、`eval_en_candidates.py`、`eval_ko_candidates.py`、`eval_lid_candidates.py`、`eval_lid_curve.py`、`eval_lid_voxlingua.py`、`lid_target_clf.py`、`eval_turbo_zhko_yue.py`、`eval_singing.py`、`make_singset.py`、`eval_noise.py`、`make_noisyset.py`、`eval_head_dropout.py`、`eval_diar_osd.py`、`eval_diar_overlap.py`、`eval_punct_4class.py` | 候補を選んだ調査の道具で、結果は `docs/eval/` に日付つきで残っている。実行時の経路も受入も読まない |
| `scripts/make_testset.py` | 合成の声をクラウドの TTS（edge-tts）で作る。クラウドを使わない方針に反し、受入も読まない |
| `scripts/make_punct_fixture.py` | 出力の `mobile/hayamimi_core/test/fixtures/punct_ja_parity.json` は commit 済みで、RP-18 はその file を読む |
| `scripts/make_punct_trainset.py`、`train_punct_ja.py`、`export_punct_4class.py`、`export_voxlingua_lid_onnx.py`、`quantize_punct.py`、`quantize_reazonspeech.py`、`ja_text_norm.py` | model を作る道具。作った model は配布元（Hugging Face、sherpa-onnx の release）から取る。学習と変換は PyTorch に依存し、Rust に移す道具が無い |
| `demo/hayamimi_scenes.py` | manim の動画の台本。描いた mp4 は `demo/remotion/public/manim/` に commit 済み |
| `tests/*.py` の 20 本 | 中身の case を Rust の test へ写してから消す（RP-19）。`tests/golden/` の音声と `golden.json` は残し、Rust の test が読む |

上の表の `scripts/` は 2 + 16 + 1 + 1 + 7 = 27 本である。

#### mobile/ と demo/

- **mobile/**（Dart、187 file）：変えない。Python ではなく、別の製品である。desktop の event と HTTP の形を読む consumer なので、RP-12 で互換を守る。Dart の comment が「どの Python から写したか」を書いた行は、R11 で Rust の module の名前に書き替える（3.9）。
- **demo/**：remotion（TypeScript）と画像は変えない。Python 2 本は上の表のとおり。

#### 評価、学習、data 作りの道具の扱い

評価と data の取得は `hayamimi-eval` に移し、受入の command はすべてこの道具で打つ。
`hayamimi-eval` は `hayamimi` の実行 file に link しない別の bin で、配る物に入れない。
学習と model の変換は repo から消し、配布元の model を使う。

### 3.3 構成

```
native/                          Cargo の workspace（既存）
├── hayamimi-relay/              訳の中継（既存、slice 1）
├── hayamimi-gpu/                CUDA 版の sherpa-onnx と ORT を開く、device を数える、provider の割り当ての log
├── hayamimi-asr/                VAD、言語判定、振り分け、認識、ITN、句読点、話者、清書
├── hayamimi/                    実行 file。取り込み、ingest、server、CLI、models fetch、config dump
├── hayamimi-ui/                 Dioxus web。dashboard、overlay、transcript
├── hayamimi-eval/               開発用の bin。stages、stream、scorecard、golden、chrf、der、fetch-set
└── xtask/                       開発用の bin。CUDA 版の sherpa-onnx の C library を build する
```

依存の向きは `hayamimi` → `hayamimi-asr` → `hayamimi-gpu` の 1 方向とする。
`hayamimi-relay` は `hayamimi` と別の process のまま、`hayamimi` の `/events` を購読する（会話字幕の設計 3.2 のまま）。
同じ process に入れない理由は、中継が 2 つの音源の認識を 1 か所で結ぶ設計を変えずに済むからで、localhost の SSE の 1 段が足す時間は `L_tr` に入る。

### 3.4 推論の engine と GPU の経路

#### 認識は自分で build した CUDA 版の sherpa-onnx

sherpa-onnx の C library を、`-DSHERPA_ONNX_ENABLE_GPU=ON -DBUILD_SHARED_LIBS=ON` で build する（S13）。
Rust の `sherpa-onnx` crate は feature `shared` で使い、build の時に `SHERPA_ONNX_LIB_DIR` にその build の `lib` を渡す（S3）。
`SHERPA_ONNX_LIB_DIR` は crate の build script が読む値で、この repo の code は読まない。
認識器、VAD、言語判定、話者の埋め込み、清書の再分離の config の `provider` は、3.5 の表で GPU とした段を `cuda`、CPU とした段を `cpu` にする。
build の手順と版の固定は 3.12 に置く。

この形では、model の推論（ORT の session の実行）が GPU で走る。
音響特徴（fbank）と探索（transducer の beam search、CTC の取り出し）は sherpa-onnx の C++ の中で走り、その置き場は sherpa-onnx の実装が決める。
音声は C の API が host の `const float *` で受ける（S11）。
段と段の間の受け渡しは sherpa-onnx の中で閉じていて、外から GPU の memory のまま渡す口は無い。
この中の段ごとの時間と、host と GPU の間の転送の時間は、R1 で計る（3.5、RP-23、RP-7）。

#### ONNX Runtime は process に 1 つ

同じ process に 2 つ目の ONNX Runtime を入れると落ちる（sherpa-onnx の issue 3261、`docs/spec/ja_pipeline.ja.md` の「ONNX Runtime の呼び出し順」）。
したがって句読点の model は、`ort`（2.0.0-rc.14）を `load-dynamic` で使い、自分で build した sherpa-onnx に入っている同じ `onnxruntime` の library を開く。
`ort` の session には CUDA の実行 provider を登録し、`IoBinding` で入力と出力を device に置く（S4）。
`ort` がその ORT の版の C API で動くかは R1 の RP-17 で確かめる。

Windows には Windows ML 用の `C:\Windows\System32\onnxruntime.dll`（1.17.1）が在る。
`sherpa-onnx-c-api.dll` の `onnxruntime.dll` の import が名前だけで解決されると、実行 file の隣の ORT ではなくこの 1.17.1 に結びつくことがあり、その run は要る ORT の API の版が無いと言って異常終了する（終了 code -1073741819）。
どの条件で System32 の側に結びつくかは特定していない（10 の H28）。
したがって Windows では次の 3 つを規則とする。

1. `sherpa-onnx-c-api.dll` を遅延読込みにする。link の引数は `/DELAYLOAD:sherpa-onnx-c-api.dll` と `delayimp.lib` の 2 本である。
2. 最初の sherpa-onnx の呼び出しより前に、実行 file と同じ directory の `onnxruntime.dll` を完全な path で読み込む。同じ名前の module が読み込み済みなら、後から来る名前だけの import はその module で解決されるからである。
3. `hayamimi-gpu` 自身と、`hayamimi-gpu` を依存に持つ全 crate の bins と tests に、1 の 2 本の link 引数を同じ形で置く。build script の link 引数はその build script を持つ crate の target にだけ届き、依存する側の crate の実行 file には届かないからである。同じ理由で `hayamimi-gpu` 自身の bins と tests も数える対象に入り、依存する crate が 0 本の workspace でも数える crate は 1 本以上ある。bins へは `cargo::rustc-link-arg-bins`、test の実行 file へは `cargo::rustc-link-arg-tests` で渡す（bins の指定は test の実行 file に届かない）。

Linux では 2 の先読みをしない。`sherpa-onnx-sys` が `libonnxruntime.so*` を別々の file として写すので、先読みした file と sherpa-onnx が読む file が別の module になり、ORT が 2 つ入るからである（R1 の実測）。
Linux の ORT の版の問い合わせは、SONAME の `libonnxruntime.so.1` で開いた module に対して行う。

RP-17 は、`onnxruntime` の module の読み込み元が実行 file と同じ directory であることを合格の条件に持つ。

**session は 1 本ずつ作る**：同じ process の中で sherpa-onnx の session（CUDA）と `ort` の session を同時に作ると、C++ の例外が Rust の境を越えて process が abort することがある（`fatal runtime error: Rust cannot catch foreign exceptions`。並列 2 の test の 9 回のうち 2 回）。
abort は fallback で受けられず、止めない形（3.15）が成り立たない。
したがって session を作る呼び出し（sherpa-onnx の全ての `create` と `ort` の `commit_from_file` ほか）は、`hayamimi-gpu` が持つ process 全体の 1 つの lock の下で 1 本ずつ行う。
推論の呼び出しは lock の外のまま並べる（abort は session を作る時に見た物で、推論の並びでは見ていない。R3b が言語判定の CPU 側の推論 8 回と GPU 側の session の作成を重ねた 20 回で abort は 0 回だった）。
lock の根拠は、lock の無い形で `rp16_lid` を並列 2 で 9 回打って 2 回の abort、lock を入れた後の同じ打ち方で 9 回とも 0 回、の実測である。
作る順が 1 本ずつになって延びるのは起動の時だけで、`L` には入らない。

#### model は fp32 か fp16 を使う

CUDA の実行 provider は `DynamicQuantizeLinear` と `DynamicQuantizeMatMul` を持たない（ORT の `OperatorKernels.md`、CUDA の節を `grep` で数えて 0。CPU の節には在る。S6）。
動的に int8 へ量子化した今の model を CUDA に渡すと、その node が CPU に落ち、host との copy が node ごとに起きる。
したがって GPU では fp32 か fp16 の版を読む。
fp16 にするか fp32 にするかは、RP-1 の誤り率を満たす中で速い方を model ごとに R1 の計測で選ぶ。
fp32 も fp16 も配布されていない model（Parakeet v3、parakeet-ja）は 10 の H1 とする。

### 3.5 段ごとの置き場

既定は GPU である。
R1 の計測（RP-23）で、GPU の経路が在る段を同じ入力で GPU と CPU の 1 stream ずつ測り、p50 と p95 を `docs/results/gpu_stages.md` に置く。
同じ計測を、llama-server に訳の依頼を流し続けた負荷の下でも 1 回取る（同じ GPU を 2 つの process が分け合う時の時間を見るため）。
置き場は、次の 4 つの根拠のどれかで決める。

1. **同じ精度の比較**：同じ精度（`gpu_stages.md` の同じ variant）の model の GPU と CPU の p50 を比べ、速い方に置く。精度の違う対では決めない。置き場を決めた variant は、表の根拠の列に「根拠 1（variant `<名>`）」の形で 1 つだけ書く。同じ段に別の variant の対（認識の `ort-encoder fp32` など）が在っても、置き場の判定には使わない。置いた側の中でどの精度の model を読むかは、その側の p50 と RP-1 の誤り率で選ぶ。
2. **転送の下限との比較**：文字列か数個の数を扱う段では、`gpu_stages.md` の GPU の行（variant `transfer-floor`）は kernel を含まない転送の往復だけの下限である。CPU の p50 がこの下限より小さければ、GPU は CPU より速くなれないので CPU に置く。
3. **縮む時間の上限**：CPU の p50 が下限より大きくても、GPU へ移して縮む時間は CPU の p50 を超えない。その上限が `L` の見積りの p50 の 1% より小さければ CPU に置く。この根拠を使う段は resample だけで、悪い方の run の 3220 µs（`gpu_stages.md` の resample の `cpu` 負荷 run 2 の行、:35）でも 1349 ms の 0.24% である。分母の `L` は R1 の段の和の見積りで、実測の `L` ではない。
4. **GPU の経路なし**：GPU で動く経路が無い段は「CPU（GPU 経路なし）」と書き、その段を作る slice が CPU の p50 を `hayamimi-eval stages` で測って `gpu_stages.md` の同じ段の CPU の行に置く。受ける式は「CPU の p50 が GPU の往復の下限より小さい」で、成り立たなければその slice が評議へ戻す。

**GPU の往復の下限**は、`gpu_stages.md` の「認識の文字列 → 訳の入力」の GPU の行の無負荷の 3 run の p50 の最小で、いまは 219 µs（3 run は 219〜232 µs、`gpu_stages.md` の「段ごとの時間」の同じ段の `cuda` `transfer-floor` 無負荷の行、:192-194）である。
根拠の列の `gpu_stages.md` の行番号は、段の名前の行（「段ごとの時間」の表）と置き場の提案の行（「置き場の提案」の表）を指す。

| 段 | いまの置き場 | 根拠、または R1 で測る物 |
|---|---|---|
| 音声の取り込み（WASAPI のマイク、loopback） | CPU（GPU 経路なし） | 根拠 4。WASAPI の capture は host の buffer に sample を渡す（S7、https://docs.rs/wasapi/latest/wasapi/ ）。R5 が、取り込みから認識器へ渡すまでの CPU の p50 を測る |
| int16 → float32、16 kHz への resample、mix | CPU | 根拠 3。CPU の p50 は無負荷 2455 µs、負荷の下 3195 µs で、GPU の行は kernel を含まない下限の 345 µs と 368 µs である（`gpu_stages.md` の「int16 → float32、16 kHz への resample、mix」の行、:27-35、:256）。GPU へ移して縮む時間の上限 2455 µs は、`L` の見積りの p50 1349 ms（「遅れ L の見積り」の「Rust 認識 cuda / 訳 cuda 無負荷」の `L` の見積りの行、:300）の 0.2% である（負荷の下も 3195 µs 対 1712 ms で 0.2%、「Rust 認識 cuda / 訳 cuda 負荷」の `L` の見積りの行、:302）。sherpa-onnx の C の API は音声を host の `const float *` で受ける（S11） |
| 音声の ring buffer と preroll（1.0 秒） | CPU | 根拠 2。CPU の p50 15〜22 µs が、GPU の下限 247〜267 µs（無負荷）より小さい（「音声の ring buffer と preroll」の行、:36-44、:257）。認識器へは host の配列で渡す（S11） |
| VAD（Silero、sherpa-onnx の `provider`） | CPU | 根拠 1（variant `fp32`）。同じ fp32 で、無負荷の p50 が GPU 388 ms に対し CPU 40 ms、負荷の下は GPU 1121 ms に対し CPU 42 ms（「VAD」の行、:45-53、:258） |
| 言語判定（whisper-tiny） | GPU | 根拠 1（variant `fp32`）。無負荷の p50 が GPU（fp32）76 ms に対し、CPU は fp32 で 132〜152 ms、int8 で 124 ms。負荷の下は GPU 94 ms に対し CPU の int8 132 ms（「言語判定」の行、:54-66、:259）。CUDA の session でも CPU に置かれる node は tiny-encoder が 31、tiny-decoder が 201 で（node の表の `tiny-decoder.onnx` と `tiny-encoder.onnx` の行、:248-249）、上の GPU の値はそれを含む |
| 認識の encoder（全 model） | CPU（int8） | 根拠 1（variant `fp32`）。sherpa-onnx の認識全体（encoder と探索）の同じ fp32 の対で、無負荷の p50 が GPU 171414 µs に対し CPU 151299 µs、負荷の下は GPU 375176 µs に対し CPU 175025 µs（「認識の encoder」の行、:67-94、:260）。CPU の中では int8 が 128615 µs で fp32 より速いので int8 を読む（int8 は CUDA に op が無く、置き場の比較には入れない。3.4）。encoder だけを `ort` で掛けると GPU 66262 µs に対し CPU 121091 µs で GPU が速いが、provider は認識器の config に 1 つで encoder と探索を別の provider に分けられない（S11 の `SherpaOnnxOfflineModelConfig`）。この置き場は測った ReazonSpeech の transducer に限る。他の認識器は R3 が同じ式で測る。CPU が速い向きはどの測りでも同じだが、差は CPU の時間に対して、上の無負荷の値で 13%（171414 対 151299 µs）、別の測り直しの認識全体の p50 で、負荷の低い run が 8%（cuda 174.3 対 cpu 162.0 ms）、load 12 の run が 2%（cuda 192.1 対 cpu 188.4 ms）と小さく、CPU の負荷が重なると CPU の側が悪化する。R7 の 2 音源同時で測り直す |
| fbank と探索（sherpa-onnx の中） | sherpa-onnx の実装のまま（host） | provider に依らず host で走る（「fbank と探索」の行、:95-112、:261）。この行は fbank だけで、探索は認識の encoder の行の合算に入っている。探索の時間は、合算から encoder だけの時間を引いて出す（10 の H11、H31） |
| 冒頭欠落の疑い判定と分割再試行 | CPU（判定）。解き直しは認識の encoder の置き場に従う | 根拠 2。判定の CPU の p50 1 µs が、GPU の下限 216〜241 µs（無負荷）より小さい（「冒頭欠落の疑い判定と分割再試行」の行、:113-121、:262） |
| ITN（漢数字 → 算用数字） | CPU（GPU 経路なし） | 根拠 4。model を持たない文字列の規則（正規表現）で、ORT も CUDA も呼ばない。R2 が 1 行の CPU の p50 を測る |
| 句読点（BERT、`ort` の CUDA） | GPU | 根拠 1（variant `fp32`）。`ort` で `punct_ja_parity.json` の `input_ids` を forward だけ掛けた同じ fp32 の対で、無負荷の p50 が GPU 8272 µs に対し CPU 44899 µs、負荷の下は GPU 11112 µs に対し CPU 41557 µs（「句読点」の行、:130-139、:264）。GPU の悪い方の run の 15398 µs（`cuda` 負荷 run 2 の行、:134）も、CPU の速い方の run の 40110 µs（`cpu` 負荷 run 2 の行、:139）より速い。CUDA の session でも CPU に置かれる node が 77 在り（node の表の `punct_bert.onnx` の行、:247）、上の GPU の値はそれを含む。入力の文字列 → token id の変換は表を引く 1 回で、CPU で行う |
| 句読点の後処理と疑問符の規則、置換辞書 | CPU（GPU 経路なし） | 根拠 4。model を持たない文字列の規則で、ORT も CUDA も呼ばない。R2 が 1 行の CPU の p50 を測る |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | CPU | 根拠 1（variant `fp32`）。同じ fp32 で、無負荷の p50 が GPU 227 ms に対し CPU 71 ms、負荷の下は GPU 268 ms に対し CPU 83 ms（「話者の埋め込み」の行、:148-156、:266） |
| 話者の速報の割り当て（cosine と重心の更新） | CPU | 根拠 2。CPU の p50 2〜3 µs が、GPU の下限 237〜251 µs（無負荷）より小さい（「話者の速報の割り当て」の行、:157-165、:267） |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | CPU | 根拠 1（variant `fp32`）。同じ fp32 で、無負荷の p50 が GPU 118 ms に対し CPU 34〜39 ms、負荷の下は GPU 205 ms に対し CPU 41〜42 ms（「清書の再分離」の行、:166-178、:268） |
| 清書の clustering（FastClustering）と全体の対応づけ | CPU（GPU 経路なし） | 根拠 4。sherpa-onnx の FastClustering は Eigen と fastcluster の C++ で、ORT も CUDA も使わない（S20）。再分離と同じ呼び出しの中で走り、分けて測る口が無い（「清書の clustering」の行、:179-182）。R4 が、FastClustering と全体の対応づけの CPU の p50 を測る |
| 訳 T と E（llama-server、CUDA） | GPU | 根拠 1（variant `CAT-Translate-3.3b-Q8_0`）。T の無負荷の p50 が GPU 253 ms に対し CPU 2858 ms、負荷の下は GPU 435 ms に対し CPU 3337 ms（「訳 T と E」の行、:183-191、:270）。測ったのは WSL の CUDA 12.0 の build の llama-server で、本番の Windows の CUDA 13.4 の build ではない。R8b が Windows の CUDA 13.4 で T を測り直し、`gpu_stages.md` の T の行を置き換える。E は測っていない |
| 認識の文字列 → 訳の入力 | CPU（文字列） | 根拠 2。CPU の p50 2 µs が、GPU の下限 219〜232 µs（無負荷）より小さい（「認識の文字列 → 訳の入力」の行、:192-200、:271）。認識の token と言語モデルの token は語彙が違い、文字列に戻して切り直すしかない。llama-server の API は文字列か token の id を受ける（S14） |
| 照合 V と単位の換算 | CPU | 根拠 2。CPU の p50 8〜9 µs が、GPU の下限 220〜251 µs（無負荷）より小さい（「照合 V と単位の換算」の行、:201-209、:272）。会話字幕の CX-3 と CS-6 の 1 行あたり 5 ms 以下も満たす |
| 字幕の配信（SSE）と描画 | CPU（GPU 経路なし）。描画は WebView2 | 根拠 4。文字列を SSE で送る段で、GPU で動く経路は無い。R5 が、確定文の event を `/events` へ書き出すまでの CPU の p50 を測る。描画の時間は会話字幕の CS-7 の中で測る |

段と段の間の転送は、R1 で発話ごとに測る。
sherpa-onnx の中の転送は、CUDA の profiler（Nsight Systems の `nsys profile --stats=true` の `cuda_gpu_mem_time_sum` と `cuda_gpu_mem_size_sum`）で、1 発話あたりの host → device と device → host の回数、bytes、時間を数える。
profiler が Windows で使えないときは WSL の同じ build で数え、そのことを結果の file に書く。

### 3.6 訳の速度

会話字幕の訳は T（訳す）→ E（自然に直す）→ V（照合）の 3 段で、T と E は言語モデルに 1 回ずつ頼む。
試し 1 の実測では、CAT-Translate-3.3b の 1 回が中央値 148 ms、p95 277 ms だった（`conversation_subtitles.ja.md` 2.3）。
T と E を直列に待つと、訳の遅れはその 2 倍になる。
速度を第一にするので、次の 6 つで `L_tr` を縮める。

1. **先に T の訳を出す**：T の応答に V を掛け、通れば先の訳として自分の重ね窓と OBS の字幕頁に出す（`status: "ok"`、`stage: "t"`）。続けて E を頼み、E の応答に V を掛けて通れば同じ `id` の行を置き換える（`stage: "e"`）。E が V に落ちたら T の訳を残す。OBS の字幕頁は表示を置き換えられるので、自分の重ね窓と同じく先の訳を出して確定の訳で置き換える。V に落ちた行（`held`）と翻訳器が答えない行（`unavailable`）は、OBS の字幕頁には何も出さない（会話字幕の設計 3.4 の「相手へ」の列と同じ）。V は 1 行につき多くて 2 回で、同じ訳には 2 回掛けない。
2. **確定の訳だけを外へ出す**：**確定の訳**は、E の訳が V を通ればその訳、外れて T の訳が通っていれば T の訳とし、E の応答を照合した後に 1 回だけ決める。中継の `translation` の event は `settled`（真偽）を持ち、確定の訳を決めた event だけが真である。E の訳が V を通れば `stage: "e"` で `settled: true`、E が V に落ちれば T の訳と同じ `text` を `stage: "t"` で `settled: true` として同じ `id` に 1 回送り直す。先の訳は `settled: false` である。画面は `settled` が真の event で行を確定の形にする。Zoom と Teams への送信と SRT は確定の訳だけを使い、1 つの `id` につき 1 回送る（会話字幕の設計 3.4、3.10、3.13）。送った字幕は相手の画面で置き換えられないからである。
3. **速報（partial）の訳**：認識の速報が更新されるたびに、その文字列を T に頼み直す（re-translation）。速報の訳は `translation` の event の `of` を `"partial"` にして見分け（会話字幕の設計 3.3 の `of` に `"partial"` を足す）、`settled` は常に偽で、外の送り先には送らない。前の訳と今の訳の最長共通接頭辞だけを確定した字形で描き、残りは薄く描く。IWSLT 2026 の Pinch-AST が同じ文字単位の最長共通接頭辞の方針を取っている（S9）。速報の依頼は 1 本だけを走らせ、新しい速報が来たら走っている依頼を打ち切る（stream の応答の接続を閉じる。llama-server は接続を閉じると 1 秒以内に生成を止めて slot を空ける。`native/hayamimi-relay/tests/r8e_server.rs` の `closing_the_stream_after_the_first_token_frees_the_slot_within_a_second` が確かめる）。`cache_prompt` を真にし、伸びた文字列の前の部分の計算を使い回す。速報の訳にも V を掛け、外れたら速報の訳は出さない。
4. **揺れを数える**：1 つの発話について、字幕に出た訳の文字列が変わった回数と、一度確定した字形で描いた接頭辞が後で消えた回数を数える（RP-25）。速度のために先に出す形が、読む人に揺れとして見える量を測るためである。
5. **並べて走らせる物**：llama-server の slot を 4 にする（mic の確定、loopback の確定、速報、E）。認識（ORT）と訳（llama.cpp）は同じ GPU を時間で分け合う。負荷の下での `L` は RP-3 で測る。
6. **llama-server の置き場**：Windows の native の CUDA 版に置く（会話字幕の設計 3.7）。model の GGUF は `%LOCALAPPDATA%\hayamimi\models\llm\` に置き、正本の WSL の `/mnt/h3-models` から写す（2.3。Windows から直に読めないため）。写した file の sha256 を同じ directory の `manifest.json` に記録する（記録で、起動の条件にしない）。hooks の `guard_commands.rs` は、`llama-` で始まる実行 file の名前を含む command を WSL の build の鮮度で refuse する（`problems` の中の `LLAMA_EXECUTABLE` の検査）。Windows の llama-server は PowerShell から起こし、WSL の shell からは起こさない（10 の H20）。

**訳の依頼の形**：中継は T と E を llama-server の `/v1/chat/completions` に、user の message 1 つ（T は model card の形「Translate the following {Japanese|English} text into {English|Japanese}.\n\n{原文}」）で頼み、`temperature` 0、`stop: ["</s>"]`、`cache_prompt: true` を付ける（S14）。
chat template は llama-server が GGUF の metadata から当てる。
`/completion` に平文の `prompt` を渡す形を採らないのは、model の chat template（`<|user|>`、`<|assistant|>` の語）が入らず、CAT-Translate を測った probe（`testdata/probe1_cat-translate-3.3b.log`、`/v1/chat/completions` で測った）と違う入力を model に渡すからである。
中継が template を自分で組む形も採らない。model を替えるたびに template を写し直すことになり、写し違いを見つける手段が無いからである。
template の token の数は 2 つの形で同じなので、1 行の訳の遅れは変わらない。
`/v1/chat/completions` が `cache_prompt` と `stop` を受けることは README の `/completion` の節の記述からの読みで、確かめていない（NOT VERIFIED。T-CS-8-001 と 002 の body の検査と、R8d の速報の訳の `cache_prompt` の測りで確かめる）。
持ち主は R8c の直しで、R8e が `L` を測る前に入れる。
受入は同じ llama-server で、probe1 の 40 行の原文を中継の T に流した訳の文字列が、同じ server に `curl` で `/v1/chat/completions` を同じ body で打った訳と全行で同じであることとする（陽性対照は `curl` の側が probe1 の log の出力と同じであること。llama.cpp の版が probe の時と違うと、そこが割れることがあるので、割れた行は数を記録し、合否は中継と `curl` の一致で決める）。
陰性対照は、今の形（`/completion` に平文の `prompt`）で同じ 40 行を流し、`curl` と違う行が 1 行以上出ることである（0 行なら、この受入は 2 つの形を見分けていないので、行を足して打ち直す）。

`--translate` の言語（今は FuguMT の en と、M2M-100 の zh、ko、es ほか）も、言語モデルの T で訳す。
ja と en の間は CAT-Translate-3.3b で訳す。
CAT-Translate-3.3b は zh、ko、es を訳せない（R8f の実走：V が止めた行が ko 50/50（全部 `form`、訳が日本語の原文のまま）、zh 37/50、es 20/50）。
したがって ja と en の外の言語は、別の言語モデルを 2 本目の llama-server（port 18081、`-np 1`）に置いて訳す。
port 8081 は同じ機械の別の process が使うので避けた。起こし方は `docs/guide/llama_other_server.ja.md` に書く。
2 本目は `--translate` に ja と en の外の言語が在る時だけ起こすので、ja と en の経路の速さは変わらない。
model は会話字幕の設計 9 の手順（held-out の順位づけ）で決め、手元の候補の TranslateGemma-4B-it Q8_0（`testdata/probe1_translategemma-4b.log`）から測る。
TranslateGemma は 55 言語を訳し（技術報告 arXiv 2601.09012、自動評価は WMT24++ の 55 言語対）、人手の評価に英語から中国語（簡体）と韓国語、自動評価に英語からスペイン語を含む。
`--translate` の表の 15 言語は、model の chat template の言語名の表にすべて在る（GGUF の metadata を `grep -a -c -F` で数えた）。
55 言語の一覧（技術報告の付録 C）は読んでいないので、zh、ko、es 以外の 10 言語（fr、de、it、pt、nl、pl、tr、id、vi、ru、uk）が学習に入っていることは NOT VERIFIED である。
2 本目は T だけで訳し、E を掛けない。TranslateGemma の template は指示文の message を受けないからである（平文の message は原文として包まれ、en-GB から en-GB への翻訳の依頼になる）。V は T の訳に 1 回掛け、通らない行は止める。
TranslateGemma の chat template は原文と訳の先の言語の code を template の変数 `source_lang_code` と `target_lang_code` で受ける。llama-server は変数を request の `chat_template_kwargs` からだけ読み、message の中に書いた code は捨てて両側を en-GB にする。中継は message に原文だけを置き、`chat_template_kwargs` に 2 つの code を置く。
受入は 3.6 の「訳の依頼の形」と同じ形で、`native/hayamimi-relay/tests/chat_probe_gemma.rs` に在る。`curl` の答えが probe1 の log の 40 行と同じ（陽性対照）、中継の答えと `curl` の答えが同じ、code を置かない形が `curl` と違う行を持つ（陰性対照）である。
2 本目の address は `live` と `eval` の `--llm-other`（既定は `http://127.0.0.1:18081`）で渡す。
起こす側は `native/hayamimi-relay/units/hayamimi-llama-other.service`（systemd の user unit）で、WSL 以外の機械では `hayamimi llama start --model <file> --port 18081 --slots 1` を打つ。
2 本目が立たないとき中継は止まらず、組の外の言語の行は `held`（訳の無い行）になり、行ごとに `translate.request.failed` が出て、最初の失敗で `translate.fallback.held` が 1 回出る。
どの候補も RP-13 の記録を超えなければ、その言語は評議へ出す（CPU の翻訳器は 1.3 のとおり移さない）。
持ち主は R8f である。
ja と en の外の言語の訳は T と V だけで、E を掛けない。V で外れた行は `held` にする。E を掛けないのは、TranslateGemma の chat template が指示の message を受けず、E の依頼の形を作れないからである（評議の判定、GO）。E が chrF に効く量は測っていない（NOT VERIFIED）。
R8f の仕組み（2 本目の server、T だけ、言語の code を付けた依頼）は、RP-13 が未通過のまま merge してよい（評議の判定）。RP-13 の値は未通過として記録する。
V を掛けない全行の chrF が RP-13 の記録に届かない言語に限って、別の model を候補に開く。開く前に、2 本の server を並べた時の GPU の memory（RP-22）と `L`（RP-3 の式）を測る。
`--translate` は訳の先の言語の組を置き換える。`--translate zh` では ja の確定文に zh の訳だけを出し、en の訳は出さない。en も出すときは `--translate en,zh` と書く。`--translate` を付けないときは、会話字幕の設計 3.1 の音源ごとの規則（mic の ja は en へ、loopback の en は ja へ）で訳す。
品質は FLEURS の chrF で、今の値（en 47.57、zh 20.19、ko 25.20、es 42.09。FuguMT と M2M-100 で取った値、RB-7）以上を RP-13 で測る。
V が止めた行は空の hyp のまま数える。利用者に見えるのは訳の無い行だからである。
同じ結果の file に、止めなかった行だけの chrF と、止めた行の数を別に書く。止めた行の数は、V が正しい訳を止める数（会話字幕の CS-24）の材料にする。
CPU の翻訳器を残さないのは、会話字幕の設計 3.6 の理由（訳の質）と同じである。

音声を入れたまま訳す end-to-end の model は採らない。
別の席の調べでは、Whisper の translate は英語への訳だけで、SeamlessStreaming は CC BY-NC で PyTorch だけで動き、Canary と Parakeet は日本語を含まず、Riva は Linux の server である（S15）。

### 3.7 音声の取り込み

| 入力 | Windows | Linux（WSL） |
|---|---|---|
| マイク | `wasapi` crate（0.26、shared mode、event 駆動） | `cpal`（0.18） |
| スピーカーの loopback | `wasapi`（loopback capture、S7） | 無し（今の Python と同じ） |
| mix | 2 本の capture を時刻で合わせて足す（今の 48 ms の窓、[-1, 1] への clip、mic を時計にする規則を写す） | 無し |
| COM の初期化（Windows） | `hayamimi` が取り込みの thread ごとに自前の scope で初期化と後始末を持ち、既に別の mode で初期化された thread と、それ以外の失敗の HRESULT を分けて扱う（今の Python と同じ規則。RP-19 の `test_audio_capture.py` の 3 行） | 無し |
| WebSocket の `/ingest` | 同じ protocol（最初の JSON の frame、以後 `pcm_s16le`）、既定の bind は `127.0.0.1` | 同じ |
| wav | `--wav`、`--no-realtime` | 同じ |

### 3.8 字幕の server と画面

server は axum で、route、query、event の JSON の形、port の既定（8833、8766）を今の Python と同じにする。
`/config` と `/reset`、`/replacements`、`/itn_overrides` の GET と POST も同じ形にする。
今の形は R0 で Python の server から録った event と応答を fixture にし、Rust の出力と key と型で比べる（RP-12）。

dashboard、OBS の overlay、transcript の 3 頁は Dioxus web（WASM）で作り、server が配る。
見た目の詰めは designer の席が持つ。
designer に渡す物は、3 頁の URL と query（`?show=final`、`?show=partial`）、今の頁の要素（速報の帯、確定の行と言語の印と話者の印と行ごとの遅れ、訳、清書の列）、mobile の `overlay_html.dart` と同じに描くという条件（`tests/test_overlay_parity.py` が今それを測っている）、速報の訳を確定の部分と薄い部分に分けて描く形、先の訳を確定の訳で置き換える形である。
会話字幕の重ね窓（`hayamimi-overlay`）は会話字幕の設計 3.12 のままとする。

### 3.9 文書と comment の扱い

| 読む者 | 扱い |
|---|---|
| `docs/eval/` の 10 file | 日付つきの実験の記録で、後から書き換えない決まり（README.ja.md の docs の決まり）。当時の command として正しいので残す。`docs/README*.md` の索引に、`docs/eval/` の Python の command は R11 より前の版で打つ物だと 1 行を足す |
| README、`docs/guide/`、`docs/spec/`、`docs/results/`、`docs/verify/`、`CONTRIBUTING.md`、`THIRD_PARTY_NOTICES.md` | R10 で Rust の command に書き替える。README は `readme-writing.md` の工程（日本語の正本 → 英語の編集 → 1 回の照合）で書き直す |
| `docs/design/` の今の設計（diarization、translate、translate_m2m、punct_ja、mobile_quantization） | ある判断に至った調査の記録で、当時の file を指すのは正しい。冒頭に、実行時の置き場が R11 で Rust の module に替わったことを 1 行足す |
| mobile の Dart の comment（写した元の Python の名指し） | R11 で、対応する Rust の module の path に書き替える。挙動は変えない |
| `docs/spec/ja_pipeline_spec.json` | `hayamimi config dump` の出力で作り直す。`config` の値は 1 つも変えない（RP-19 の 1 行） |

完了の条件は、`git ls-files | xargs grep -lE 'scripts/[a-z_0-9]+\.py'` の当たりが `docs/eval/` と `docs/design/` の下だけになることである。

### 3.10 model の取得

`hayamimi models fetch` が、今の `download_models.py` と同じ組（既定、`--minimal`、`--en-parakeet-v2`、`--punct-4class`）を取る。
3.5 で GPU とした段の model は fp32 か fp16 の版を取り（3.4）、CPU とした段の model は 3.5 の表の精度の版（認識は int8）を取る。
取得先は sherpa-onnx の GitHub release と Hugging Face で、置き場は `models/`（git 管理外）のままとする。
各 file の sha256 と bytes は `models/manifest.json` に記録するが、取得と起動の可否の条件にしない（外が更新する物の版を gate にしない）。
取得の失敗は前の file を残し、一時 file に書いてから rename で入れ替える。

### 3.11 GPU の無い機械と CPU への fallback

GPU が無い機械でも字幕を止めない。
GPU に置いた段（3.5 の表で「いまの置き場」が GPU の段）を、その段ごとに CPU の実行 provider へ落として動き続ける。
黙って遅くはしない。
落ちたことは log の event と字幕の画面の両方に出す。

**確かめ方**は 2 段で、起動時に 1 回だけ行う。

1. **device を数える**：`hayamimi-gpu` が CUDA の device の数を数える（RP-17 の `cuda_devices` と同じ関数）。0 か、driver か CUDA の runtime が読めなければ、GPU に置いた段は session を作る前に全部 CPU にする。理由は `no_device` か `driver_unreadable` である。
2. **段ごとに session を作る**：device が 1 以上なら、GPU に置いた段の session を `cuda` で作る。作るのに失敗した段と、`gpu.session.created` の実際の provider が `cuda` でなかった段は、その段だけを `cpu` で作り直す。理由は `session_failed` か `provider_mismatch` である。

device を数えるだけで決めないのは、device が在っても CUDA の実行 provider が作れない場合（cuDNN が読めない、版が合わない）があるからである。
session を作るだけで決めないのは、device が 0 のときに段ごとの失敗を待つと起動が遅くなり、理由も段ごとに割れて読みにくいからである。

**段ごとの落ち先**：

| 段 | GPU の形 | CPU に落ちた形 | 持ち主の slice |
|---|---|---|---|
| 句読点（BERT） | `ort` の CUDA の実行 provider | 同じ model（fp32）を `ort` の CPU の実行 provider で動かす | R2 |
| 言語判定（whisper-tiny） | sherpa-onnx の `provider` が `cuda` | 同じ model（fp32）を `provider` `cpu` で動かす | R3 |
| 訳 T と E（llama-server） | Windows の native の CUDA 13.4 の build | 同じ GGUF を `-ngl 0`（`--n-gpu-layers 0`）で動かす。CUDA の build が device 0 で起動できなければ CPU の build（`llama-<tag>-bin-win-cpu-x64.zip`）で起こす（10 の H32） | R8b |

認識（ReazonSpeech、CPU int8）、VAD、話者の埋め込み、清書の再分離は 3.5 で CPU に置いた段なので、fallback の対象に入らない。
落ちた段は、GPU の段と同じ model を読むので、誤り率は GPU の run と同じ値を期待する（10 の H33）。

**出す物**：

- `gpu.device.unavailable` を warn の log の event で 1 回出す（`error_code` は `HAYAMIMI_GPU_DEVICE_UNAVAILABLE`、`reason` は `no_device` か `driver_unreadable`）。device が 1 以上なら出さない。
- `gpu.fallback.cpu` を、CPU に落ちた段ごとに warn の log の event で 1 回出す（`error_code` は `HAYAMIMI_GPU_FALLBACK`、`stage` は段の名前、`reason` は上の 4 つのどれか）。
- 字幕の server は、落ちた段ごとに `/events` へ 3.15 の `fallback` の event（`to` は `cpu`）を送る。dashboard と overlay はこれを読んで「CPU で動いている」印を出す（3.15）。
- 利用者の locale の文で、落ちた段と理由と、確かめる command（`nvidia-smi`）を stderr に 1 回出す。
- 終了 code は 0 のまま、字幕を出し続ける。

CPU に落ちた run の `L` は、GPU の run の合否（RP-3）に入れず、RP-28 の別の行で測る。
GPU 以外の資源（model の file、GPU の memory、llama-server、外の送り先、port、取り込みの装置、推論の library）が無いときも、同じ形で落とす（3.15）。

### 3.12 CUDA 版の sherpa-onnx の build

手順書は `docs/guide/build_gpu.ja.md`（R1 の席が書く）に置き、手順は `cargo run -p xtask -- sherpa-gpu` の 1 本で打てる形にする。
xtask は次の版を引数の既定に持ち、build の後に実際の版を `native/target/sherpa-gpu/<os>/build.json` に書く。
版は記録で、build の可否の条件にしない（外が更新する物の版を gate にしない）。

| 項目 | 既定 | 根拠 |
|---|---|---|
| sherpa-onnx | tag `v1.13.8`（Rust crate の版と同じ。crate の build script が同じ版の C の API を前提にする） | S3 |
| ONNX Runtime | 1.31.0 の CUDA 13 版。両 OS とも Microsoft の release の asset で、Windows は `onnxruntime-win-x64-gpu_cuda13-1.31.0.zip`（309,484,217 bytes、sha256 `d332ccf8…552f5`）、WSL は `onnxruntime-linux-x64-gpu_cuda13-1.31.0.tgz`（244,574,938 bytes、sha256 `2867dec9…81135`）。bytes と sha256 は記録で、取得の可否の条件にしない。展開した directory を `SHERPA_ONNXRUNTIME_INCLUDE_DIR` と `SHERPA_ONNXRUNTIME_LIB_DIR` で cmake に渡す。1.28.3 の CUDA 13 版は退路で、両 OS とも Microsoft の asset（`onnxruntime-win-x64-gpu_cuda13-1.28.3.zip`、`onnxruntime-linux-x64-gpu_cuda13-1.28.3.tgz`）である。その build を `native/target/sherpa-gpu-ort1283/` に置く（下の戻し方） | S16、S19 |
| CUDA | 13.4 の 1 本で、両 OS とも 13.4.3 を現行にする。13.4.3 は両 OS とも NVIDIA の redistrib の zip（tar）を展開して入れ、各部品の sha256 を redistrib の json と照合する。置き場は Windows が `%USERPROFILE%\cuda-redist\v13.4.3`、WSL が 13.4.1 の `/usr/local/cuda-13.4` の横である。Windows でも installer を使わないのは、installer が既存の `v13.4` の directory を上書きし、前の版へ戻せなくなるからである。退路は Windows の 13.4.59（nvcc の版）の `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.4` と WSL の 13.4.1 の `/usr/local/cuda-13.4` で、消さずに残す。WSL に入っている 13.3 も消さずに並べる。xtask には `--cuda-dir` で 13.4.3 の directory を渡し、`/usr/local/cuda` の向きと PATH の順に依らない。llama-server も 13.4 の build で、認識と訳の CUDA が揃う。driver は 610.88 | S16、S17、`nvidia-smi`（2026-10-13） |
| cuDNN | 9.27.0.42 の CUDA 13 版（Windows は展開済み） | S5、評議の席の実測 |
| GPU の arch | `sm_120`（compute capability 12.0、`nvidia-smi --query-gpu=compute_cap`） | 実測（2026-10-13） |
| cmake の option | `-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON -DSHERPA_ONNX_ENABLE_GPU=ON`、TTS を外す option（`no-tts` の archive と同じ組） | S13 |

**ORT を CUDA 13 版の 1.31.0 にする理由**：sherpa-onnx v1.13.8 の cmake は、何も渡さないと ORT を既定の CUDA 12 版（`gpu_cuda12-1.28.2`）で取り、「Requires cuda 12.x, cudnn 9.x」と書く（`cmake/onnxruntime-win-x64-gpu.cmake` の 22 行目と 23 行目。S16）。
ただし同じ cmake の `onnxruntime.cmake` は、環境変数 `SHERPA_ONNXRUNTIME_INCLUDE_DIR` と `SHERPA_ONNXRUNTIME_LIB_DIR` が在れば、その directory の `onnxruntime.dll` と `onnxruntime.lib` を使い、既定の版を取らない（156〜178 行目。S16）。
ORT 1.31.0 には Microsoft の公式の CUDA 13 版の build が Windows と Linux の両方に在る（S19）。
`ort` 2.0.0-rc.14 の既定の C API の版は ORT 1.31 で、1.31 より前の ORT では既定を外して古い API の feature を選ぶ別の道になる。
したがって xtask は 1.31.0 の asset を展開し、この 2 つの環境変数で指して cmake を打つ。
sherpa-onnx v1.13.8 の C++ は、1.31.0 に対して直さずに build できる（R1 の実測）。
CUDA は 13.4 の 1 本になり、llama-server の 13.4 の build と揃う。
この 2 つの値は cmake が読む値で、この repo の Rust の code は読まない。

**llama.cpp は CUDA 13.4 の build を使う**：llama.cpp の release（b11583、2026-10-13）の Windows x64 の CUDA の build は `cuda-12.4` と `cuda-13.4` の 2 つで、12.4 は `sm_120` を持つ最初の版の 12.8 より古い（S17）。
13.4 の build は認識の側と CUDA の版が揃い、入れる CUDA が 1 本で済む。
したがって `llama-<tag>-bin-win-cuda-13.4-x64.zip` と `cudart-llama-bin-win-cuda-13.4-x64.zip` を使う。

sherpa-onnx の C++ は CUDA の kernel を持たず、GPU の計算は ORT の CUDA 実行 provider の library が行う。
したがって `sm_120` が効くのは ORT の側である。
退路の ORT v1.28.3 の CUDA 13 版の `libonnxruntime_providers_cuda.so` を、CUDA 13.4 の `cuobjdump` で数えた（実測、2026-10-14。WSL の退路の build が `build.json` の `provider_cubin` と `provider_ptx` に書いた値）。
1.31.0 の同じ library の件数は、この文書では数えていない（NOT VERIFIED。xtask が `build.json` に書く値を RP-24 が読む）。
`--list-elf` の機械語は、`sm_75` が 187、`sm_80` と `sm_86` と `sm_89` が各 235、`sm_90a` が 239、`sm_100a` が 235、`sm_120a` が 203 で、`sm_120` は 0 件である。
`--list-ptx` は `sm_120` の PTX が 235 件である。
`sm_120a` の ELF は 203 件で、`sm_120` の PTX の 235 件より少ない。
したがって compute capability 12.0 の GPU で全 kernel が機械語で当たるとは言えず、`sm_120a` の機械語がその GPU で実際に使われるかも確かめていない（ASSUMPTION）。
機械語の当たらない kernel は、driver が初回に PTX から変換し、変換の結果を JIT の cache に置く（S18）。
その初回の遅れは 10 の H27 で測る。
xtask は build のたびに同じ数を取り、`build.json` に書く。

**Windows（msvc）と WSL で同じ build を作る**：xtask は同じ版と同じ option で、Windows では MSVC の toolchain（Visual Studio の Build Tools の `x64 Native Tools` の環境）と Windows の CUDA toolkit 13.4、WSL では gcc と WSL の CUDA toolkit 13.4 で cmake を打つ。
同じ build だと言えるのは、`build.json` の sherpa-onnx の commit、ORT の版、build に使った CUDA の主版（13）、cuDNN の主版（9）、`sm_120` 系の判定が 2 つの OS で一致するときである（RP-24）。
**`sm_120` 系の判定**は、`sm_120` か `sm_120a` の機械語が 1 件以上、または `sm_120` の PTX があり RP-17 が通った記録が在ることである。
CUDA の副版は記録し、一致を求めない。

**R1 の前に要る環境と戻し方**：

| 置き場 | 物 | 入れた状態 | 戻し方 |
|---|---|---|---|
| Windows | Visual Studio Build Tools 2022 | 17.14.41、入っている | `winget uninstall` で外す |
| Windows | CUDA toolkit 13.4.3 | redistrib の zip を `%USERPROFILE%\cuda-redist\v13.4.3` に展開済み（9 部品の sha256 が redistrib の json と一致）。sherpa-onnx と ORT と llama-server が使う。退路の 13.4.59 は `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.4` のまま | `%USERPROFILE%\cuda-redist\v13.4.3` を消す。13.4.59（退路）は installer で入れた物で、外すときだけ `winget uninstall` |
| Windows | cuDNN（CUDA 13 版） | 9.27.0.42、展開済み | 展開した directory を消し、PATH に足した行を戻す |
| Windows と WSL | ORT 1.31.0 の CUDA 13 版 | Windows は `onnxruntime-win-x64-gpu_cuda13-1.31.0.zip`、WSL は `onnxruntime-linux-x64-gpu_cuda13-1.31.0.tgz` を、xtask が `native/target/sherpa-gpu/` の下に展開する | 展開した directory を消す |
| WSL | ORT 1.28.3 の CUDA 13 版（退路） | build の本体は `~/.local/share/hayamimi/build/ort1283`（`linux` の下に `build.json`、`install`、`rp17_passed.json`）で、`native/target/sherpa-gpu-ort1283/linux` がそれを指す symlink。RP-17 を通してある | 退路が要らなくなったら、`ort1283` と symlink を消す |
| Windows | ORT 1.28.3 の CUDA 13 版（退路） | 作っていない（H39）。`native/target/sherpa-gpu-ort1282/` は ORT 1.28.2 の build で、RP-24 が比べる 1.28.3 ではない | 1.28.3 の build を作ったら `sherpa-gpu-ort1282/` を消す。退路が要らなくなったら 1.28.3 の directory を消す |
| Windows と WSL | 退路と再現の build | `native/target/sherpa-gpu/` は 1.31.0 と CUDA 13.4.3 の現行、`sherpa-gpu-repeat/` は同じ版の 2 回目の build（RP-24 の陽性対照）、`sherpa-gpu-cuda1341/` は CUDA 13.4.1 の build（CUDA の退路） | 要らなくなった directory を消す。現行の `sherpa-gpu/` は消さない |
| WSL | WSL の build の置き場 | build の本体は `~/.local/share/hayamimi/build/` の下の 3 本である。現行（CUDA 13.4.3）の `cuda1343` と、その 2 回目の `cuda1343-repeat` は、どちらも `linux`、`ort`、`src-v1.13.8`、`downloads` を持つ。CUDA 13.4.1 の build の `cuda1341` は、その build の `linux` の中身（`build.json`、`install`、`rp17_passed.json`）を directory の直下に持つ（`build.json` の `cuda_version` は 13.4.1 の nvcc の版の 13.4.59）。`native/target/sherpa-gpu/linux`、`sherpa-gpu-repeat/linux`、`sherpa-gpu-cuda1341/linux` は、それぞれ `~/.local/share/hayamimi/build/cuda1343/linux`、`cuda1343-repeat/linux`、`cuda1341` を指す symlink である。同じ directory の `redist-sums` に、CUDA 13.4.1、13.4.3、cuDNN 9.27.0.42 の redistrib の部品の一覧と sha256 を置く。`~/.cache` には何も置かない。取得した archive と cargo の `target` は作り直せるので残さない。build の本体を `~/.cache` の外に置くのは、`~/.cache` が掃除の対象になる置き場で、消えると現行の build が切れるからである。Windows からは WSL が作った symlink を辿れない（RP-24 の os error 1920） | 消し方は、現行の `cuda1343` を消さない。`cuda1343-repeat` と `cuda1341` は要らなくなったら directory ごと消し、指していた symlink も消す。全部を戻す command は `docs/guide/build_gpu.ja.md` に在る |
| Windows | CUDA toolkit 12.9.1 と cuDNN の CUDA 12 版（入っていれば） | 使わない | R1 の最初の手番で、CUDA 13 版の ORT で sherpa-onnx が build でき RP-17 が通ったら、Windows の「アプリ」から 12.9 の部品だけを外し、cuDNN の CUDA 12 版の展開先を消す |
| Windows | CMake、Ninja | 4.4.4、1.13.2、入っている | `winget uninstall` で外す |
| Windows | rustup（msvc の toolchain） | 1.29.1、入っている | `winget uninstall` で外す（`%USERPROFILE%\.rustup` と `%USERPROFILE%\.cargo` も消える） |
| Windows | Git | 2.55、入っている | `winget uninstall` で外す |
| WSL | CUDA toolkit 13.4.3 | redistrib を 13.4.1 の `/usr/local/cuda-13.4` の横に展開済み（9 部品の sha256 が redistrib の json と一致）。13.4.1（退路）と 13.3 は消さずに並べ、`/usr/local/cuda` は 13.3 を指したまま。xtask には `--cuda-dir` で 13.4.3 の directory を渡す | 展開した 13.4.3 の directory を消す。退路の 13.4.1 が要らなくなったら `/usr/local/cuda-13.4` を消す（外す設定は無い。手順は `docs/guide/build_gpu.ja.md`） |
| WSL | cuDNN 9（CUDA 13 版） | 9.27.0.42 を NVIDIA の redistrib から `/usr/local/cudnn-9.27.0.42-cuda13` に展開済み（redistrib の json の sha256 と照合） | `/usr/local/cudnn-9.27.0.42-cuda13` を消す（手順は `docs/guide/build_gpu.ja.md`） |

入れた物の版は `build.json` に記録する。
戻した後に何も残っていないかは、`winget list` と Windows の「アプリ」に上の名前が出ないことで確かめる。
12.9 の部品を外した後に、13.4 の `nvcc --version` と `nvidia-smi` が外す前と同じ出力であることも同じ手番で見る。

**ORT 1.31.0 が動かないときの戻し方**：どちらかの OS で ORT 1.31.0 の build か RP-17 が落ちたら、2 つの OS を両方とも退路の ORT 1.28.3 の CUDA 13 版に戻す（Windows は `onnxruntime-win-x64-gpu_cuda13-1.28.3.zip`、WSL は `onnxruntime-linux-x64-gpu_cuda13-1.28.3.tgz`。どちらも Microsoft の asset）。
どちらの OS も xtask に `--ort-version 1.28.3` と `--out-dir native/target/sherpa-gpu-ort1283` を渡して build し、`SHERPA_ONNX_LIB_DIR` をその install の `lib` に替えて繋ぎ直す。WSL ではこの build を作って RP-17 を通した。Windows ではまだ作っていない（H39）。
CUDA 13.4.3 の側だけが落ちたときは、CUDA の退路の `native/target/sherpa-gpu-cuda1341/`（13.4.1 の build）に同じ形で繋ぎ直す。
片方だけを戻すと、RP-24 が ORT の版の不一致で落ちるためである。

**CUDA 13 版の ORT が動かないときの戻し方**：1.28.3 の CUDA 13 版でも sherpa-onnx の build か RP-17 が通らなければ、12.9.1 は外さない。
2 つの環境変数を渡さずに cmake を打ち直し、sherpa-onnx の既定の CUDA 12 版の ORT と、CUDA 12.9.1 と cuDNN の CUDA 12 版で build する。
12.9.1 が入っていなければ、NVIDIA の local installer（`cuda_12.9.1_576.57_windows.exe`、https://developer.download.nvidia.com/compute/cuda/12.9.1/local_installers/cuda_12.9.1_576.57_windows.exe 、HTTP 200、3,562,982,664 bytes、2026-10-13 に確かめた）で部品を指定し、driver を除いて入れる。
どちらの形で build したかは `build.json` に記録し、RP-24 は記録した主版で 2 つの OS を比べる。
WSL の側で CUDA 13 版の ORT が落ちたときも、Windows の側で落ちたときも、2 つの OS を両方とも CUDA 12 版の ORT に戻す（WSL は保守者の `gpu_cuda12-1.28.2-patched` と CUDA 12 系と cuDNN の CUDA 12 版。入れる前に消し方を手順書に書く）。片方だけを戻すと、RP-24 が CUDA の主版の不一致で落ちるためである。

**Rust の target**：Windows では `x86_64-pc-windows-msvc` で、Windows の上で build する。
`hayamimi-relay` も同じ target に移す（会話字幕の設計 3.8 と 6 の `<relay>`）。

**実行時に要る file**：自分で build した `sherpa-onnx-c-api` と `onnxruntime` と ORT の CUDA の provider の library、CUDA の runtime と cuBLAS と cuDNN の DLL（WSL では `.so`）を、実行 file と同じ directory に置く。
Windows は実行 file の directory を DLL の探索に含めるので、置くだけで読まれる。
Linux の動的 linker は実行 file の directory を探さないので、実行 file に `$ORIGIN` の RPATH を持たせる（link の引数 `-Wl,-rpath,$ORIGIN` と `-Wl,--disable-new-dtags`）。
RUNPATH（`--disable-new-dtags` を付けない既定の形）では足りない。RUNPATH は実行 file が直に依る library の探索にしか効かず、ORT の CUDA の provider が後から開く cuBLAS と cuDNN には効かないので、それらが system の CUDA 13.3 から読まれるか、見つからずに abort するからである（R1 の実測）。
確かめ方は 2 つで、`readelf -d` に `RPATH` の行が在り値が `$ORIGIN` であることと、`LD_LIBRARY_PATH` を置かない run で RP-17 が通り、`/proc/<pid>/maps` の cuBLAS、cudart、cuDNN、ORT がすべて実行 file の directory から読まれることである。
置く物の一覧と sha256 は `build.json` に書く。

### 3.13 log

`observability-and-log-monitoring.md` の JSONL を stdout に出す。
event の名前は `<domain>.<action>.<outcome>` の 3 つの要素にする。

| event | いつ | 項目 |
|---|---|---|
| `asr.utterance.timed` | 発話ごと | `id`、`vad_ms`、`lid_ms`、`decode_ms`、`punct_ms`、`itn_ms`、`l_dec_ms`（記録だけ） |
| `asr.final.published` | 確定文の event を `/events` へ出し終えた時（debug） | `id`、`mono_us`（process の単調な時計の µs） |
| `speaker.embedding.started` | その確定文の話者の埋め込みを始める時（debug） | `id`、`mono_us` |
| `gpu.session.created` | session を作るたび | model の名前、要求した provider、実際に使った provider |
| `gpu.session.placed` | `--gpu-diagnose` で起こしたとき | ORT が CUDA に置いた node の数と CPU に置いた node の数 |
| `asr.model.loaded` | 認識器を memory に読み込んだとき（info） | `model`（tier の名前）、`load_ms` |
| `asr.model.evicted` | `--max-resident` の上限のため、最も使われていない認識器を手放したとき。新しい認識器を作る前に出す（info） | `model` |
| `asr.lang.unknown` | SenseVoice の結果の `lang` の tag を読めないとき。言語は不明として続ける（warn） | `error_code` |
| `gpu.memory.unreadable` | GPU の memory を読めないとき。予算なしで続ける（warn） | `error_code` |
| `model.fallback.substitute` | 認識器が使えず、別の認識器が答えるとき（warn） | `requested`、`used`、`reason` |
| `model.fallback.chunks` | 発話の検出器が使えず、長い音声を一定の長さで切るとき（warn） | `reason` |
| `gpu.device.unavailable` | 起動時に CUDA の device が 0 か、driver が読めない（warn） | `error_code`、`reason` |
| `<domain>.fallback.<落ち先>` | 3.15 の表の場面で落ちたとき、段ごとに 1 回（warn） | `error_code`（`HAYAMIMI_<DOMAIN>_FALLBACK`）、`stage`、`reason` |
| `<domain>.fallback.cleared` | 落ちた資源が戻って元の道へ戻したとき（info） | `stage` |

本文、音声、path は log に書かない。

### 3.14 言語の切り替え（i18n）

log の `message`、error、CLI の使い方、画面の文言は、locale で切り替える。
locale は OS の値（Windows は利用者の UI の言語、Linux は `LANG` と `LC_ALL`）を `sys-locale` crate で 1 回読み、既定は英語とする。
locale は実行の環境が意味を決める値で、この repo の設定ではないので、起動時に 1 回読む（`rust_guide_for_agents.md` の環境変数の節の範囲の外）。
文言は `hayamimi-relay` と同じ形の `strings.<locale>.toml`（en と ja）に置き、code に自然言語を直書きしない。
`hayamimi-ui` の画面の文言も同じ表から取る。
字幕の event の JSON の key と値（`final`、`lang` ほか）は wire 形式なので切り替えない。

### 3.15 止めない形（fallback）

`hayamimi` は、要る物が無いことを理由に起動を止めず、実行中にも止まらない。
どの場面も、同じ 3 つの手で扱う。

1. **検出する**：起動時と、その資源を使う時に確かめる。
2. **代わりの道へ落とす**：CPU、小さい model、機能を縮めた形のどれかで動き続ける。
3. **段ごとに出す**：落ちた段ごとに、log の event を 1 回、`/events` に `{"type": "fallback", "stage": ..., "reason": ..., "to": ...}` を 1 回送る。server は `fallback` の event と `session_start` の event を送った順に取っておき、遅れて接続した client と接続し直した client には、取っておいた event を送った順に最初にまとめて送ってから、以後の event を流す。fallback は起動時の確かめで決まり `session_start` より先に送るので、遅れて接続した client も `fallback` の後に `session_start` を受ける。資源が戻って `{"to": null}` を送った時は、その段の取っておいた `fallback` を取り除き、`{"to": null}` は取っておかない（遅れて接続した client は効いている fallback だけを受け、取っておく event が溜まらない）。dashboard と overlay は効いている fallback の印を出す（見た目は designer の席が持つ。3.8）。OBS の配信の画面に映る overlay も印を出し、映したくない運用では OBS の側で source を切り取る（overlay の `?show=partial` は印を出さない）。stderr にも利用者の locale の文で 1 回出す。

資源が戻ったとき（device、llama-server、送り先、装置）は、使う時の確かめで戻ったことを検出し、元の道へ戻して `<domain>.fallback.cleared` と `{"type": "fallback", "stage": ..., "to": null}` を 1 回出す。
GPU の段は起動時に置き場を決めるので、実行中には戻さない。

| 場面 | 検出 | 落ち先 | log の event（`reason`） | 持ち主の slice |
|---|---|---|---|---|
| CUDA の device が無い、段の CUDA の session が作れない | 3.11 の 2 段 | その段を CPU の実行 provider で動かす（3.11） | `gpu.device.unavailable`、`gpu.fallback.cpu`（`no_device`、`driver_unreadable`、`session_failed`、`provider_mismatch`） | R2（句読点）、R3（言語判定）、R8b（訳） |
| GPU の memory が上限（RP-22 の 14,000 MiB）に届く | 1 秒ごとに `nvidia-smi` と同じ値を `hayamimi-gpu` で読む | GPU に置いた段を、句読点、言語判定、訳（R8b の形）の順に CPU へ落とす。認識器は CPU に置くので GPU から外す物ではなく、CPU に常駐させる数を `--max-resident`（既定 3）で縛る（LRU。8 の「07:00 の計画」の R3 の着手の 3） | `gpu.fallback.cpu`（`memory_budget`） | R3 |
| model の file が無いか読めない | session を作る時 | 同じ段の小さい model（`--minimal` の組）へ落とす。それも無ければその段を外して縮めて動かす（句読点は規則の後処理だけ、言語判定は `--lang` の言語に固定、話者は印なし）。認識器は、頼んだ tier の既定の代わり（Parakeet v2 なら v3）、次に ReazonSpeech、SenseVoice、Parakeet v3、Paraformer、Omnilingual の順で読める物へ落とす（`scripts/asr_engine.py` の `_get_with_fallback` と同じ順）。parakeet-ja が無ければ second opinion を切る | `model.fallback.smaller`、`model.fallback.substitute`（認識器。`requested` と `used` を持つ）か `model.fallback.disabled`（`missing`、`unreadable`） | R2（ja の経路）、R3（振り分けの認識器と言語判定）、R4（話者） |
| 推論の library（sherpa-onnx と ORT）が開けない | 最初の sherpa-onnx の呼び出しの前（3.4 の先読み） | 認識の段を外し、server と画面と `/ingest` だけで動かす | `asr.fallback.disabled`（`library_unavailable`） | R2 |
| llama-server に届かない | 依頼の時と、5 秒ごとの `/health` | 訳を保留し、原文の字幕を出し続ける（会話字幕の設計 3.6 の `hold_reason: unavailable`） | `translate.fallback.held`（`unreachable`） | R8b（依頼の時の検出）。5 秒ごとの `/health` は 10 の H46 |
| `--translate` に、T の依頼の形（3.6 の「訳の依頼の形」）の言語の表に無い言語が在る | `hayamimi-relay live` の起動の時と、`eval` の pairs を読む時 | その言語だけを訳さず、ほかの言語を訳し続ける。`live` では表に無い言語ごとに log の event を 1 回、中継の `/events` に `{"type":"fallback","stage":"translate","reason":"unsupported_target","to":"skipped"}` を 1 回出し、取っておいて遅れた client にも送る | `translate.fallback.skipped`（`unsupported_target`、`error_code` は `HAYAMIMI_TRANSLATE_FALLBACK`） | R8f |
| 外の送り先（Zoom、Teams）に届かない | 送る時 | 送れなかった確定の訳を SRT と自分の重ね窓に残し、届いたら次の行から送り直す | `sink.fallback.local`（`unreachable`） | R8c |
| port（8833、8766）が使えない | bind の時 | 次の番号から 10 個までの空いた port を使い、実際の URL を出す | `server.fallback.port`（`in_use`） | R2（`/events`）、R5（全 route） |
| 取り込みの装置が無い（loopback の出力の装置、マイク） | capture を始める時と、装置の抜けの通知 | 在る方の音源だけで動かす。両方無ければ `/ingest` と `--wav` を待つ形で動き続ける | `capture.fallback.single` か `capture.fallback.none`（`device_missing`） | R5 |

`fallback` の event の `stage` の値は、`hayamimi` が `source`、`punct`、`recognizer`、`vad`、`asr`、`events`、`replace`、`speaker`、`ingest`、`capture`、`translate`、`lid`、`second_opinion` の 13 個、中継が `translate` と `sink` である。
`reason` の値は、`hayamimi` が `bad_value`、`no_device`、`driver_unreadable`、`session_failed`、`missing`、`unreadable`、`memory_budget`、`library_unavailable`、`in_use`、`device_missing`、`provider_mismatch` の 11 個である（`unreachable`、`unsupported_target` は中継の場面で使う）。
値を足す slice は、この 2 つの一覧を同じ commit で直す。
一覧と code（`native/hayamimi/src/notice.rs` の `STAGES` と `REASONS`）を突き合わせる受入は RP-30 で、test は R10 が作り（docs だけを読む rp08 と同じ形）、case 行は test-designer の席へ回す（セラが運ぶ）。
`models fetch` の `models.fallback.previous` は、その command の log の event で、`/events` の `fallback` の message ではないので、この 2 つの一覧に入らない。

hook と guard（`guard_commands.rs` ほか）は開発の安全の gate で、この節に入らない。
設定の file（置換辞書、ITN の上書き、`/config`）が壊れていれば前の内容を使い、前の内容も無ければ既定の設定で動かして `config.fallback.default` を出す。
既定の設定の値は `hayamimi config dump` の値と同じである（RP-19）。

### 3.16 主指標 `L` の差の詰め（R8h）

R8e の run では、Rust の `L` の p50 が Python より約 229 ms 遅く、差のほとんどは「訳」の段（`final` から最初の訳の event まで。Python の p50 212 ms と p95 1019 ms に対し、Rust は 439 ms と 1526 ms）に在る（原因の席の打ち直しで、設計の席は確かめていない。NOT VERIFIED）。
T の訳が V を通った行の `L` の p50 は Python と同じで、遅いのは T の訳が V に落ちた行（93 行中 45 行）である。
この行では、中継が E の応答とその V を待ってから最初の event を出すので（3.6 の 1。先の訳は T が V を通った時だけ出す）、E の 1 回の時間がそのまま `L` に載る。
同じ run の測り方にも欠陥が 2 つ在る。
93 行中 31 行で、`held` の空の訳を字幕が出たと数えていた（3.1 の `t_tr` の条件の前の形）。
遅れて着いた `final` の 8 標本が次の発話の窓に入り、母数から落ちていた。
R8e の記録を 3.1 の数え方で数え直すと、Rust の `L` は p50 1710 ms から 1533 ms、p95 3145 ms から 2752 ms に下がり、母数は 93 から 67 に減る（試験の席の `jq` の数え直し。設計の席は確かめていない。NOT VERIFIED）。
下がるのは、`held` の行は E の後に出る遅い行で、数え直すと訳の無い標本として母数の外へ出るからである。
数え直した値も Python（p50 1481 ms、p95 2008 ms）を超える。
したがって、2 つの欠陥は Rust の `L` を大きく見せる向きに効いており、V が止める行が増えるほど、訳の取れた標本だけの `L` は小さく出る。
この向きを合否で捕まえるために、RP-32 は訳の取れた標本の割合を合否に入れる。
Python と Rust を別の日時に、負荷を記録せずに測ってもいる。

R8h は次の 4 つを、この順に持つ。
主指標は `L` の p50 と p95 で、受入は「同じ負荷で交互に測った Python 以下」（RP-32）である。

1. **数え方を直す**（RP-31）：3.1 の `t_tr` の条件（`status` が `ok`、`text` が空でない）と、`id` と `t1_s` による結び方を `hayamimi-eval stream` の集計に入れる。設計を動かさない。字幕に出ない訳の時刻を `L` から外す直しで、2 から 4 の値はすべてこの数え方で読むので、最初に入れる。
② は B（R8f の V の直し）だけを進め、A は ④ の run と H6 の確認が揃った時にだけ作る。③ は作らず、RP-34 の合否列を `decode_ms` から `L_asr − decode_ms` に改め、待ちの log と GPU の記録を ④ の前に入れる（評議の判定。② は下の 2、③ は 3、④ は 4 を指す）。

2. **T が V に落ちた行の最初の訳を早める**（RP-33）：B を進める。A は、4 の run の後に、次の 4 つを全部満たす時にだけ作る（接続を閉じると生成が止まること（H6 の確かめ）は R8e の test で通った）。2 音源が同時に話した時の T 2 本、E0 2 本、速報、E が slot 4 本に収まる（収まらなければ slot を上げて RP-22 を測る）。T が V を通った行の `L` が悪くならない。E0 にも V を掛け、閾値を動かさずに RP-35 を通す。E0 が答えなければ直列の E へ落とし、3.15 の形で event を出す。今 A を作らないのは、`feat/r8f-verify` が main に入っておらず中継の同じ関数を 2 席が触ることになり、E0 の指示が `edit_prompt` に無い新しい prompt だからである。4 は E2E の波の中で打つので、4 の後に作る A は 07:00 に入らない。
   - **A（並べて投げる）**：T と同時に、原文だけを渡し E の指示で訳させる依頼（以下 E0）を 1 本投げる。T が V を通れば今の形（先の訳を出し、T の訳を入れた E で置き換える）で進み、E0 の接続を閉じる。T が V に落ちれば、E0 の応答に V を掛け、通れば確定の訳として出す。ただし、確定文 1 行の依頼が 2 本になり、slot 4 本（3.6 の 5）のうち E の slot を E0 が使う。2 音源が同時に話すと T 自身の待ちが延びる見込みがあり、その量は測っていない（10 の H43）。会話字幕の CS-25 の「E の依頼に T の訳が入る」が T が落ちた行で成り立たなくなるので、会話字幕の設計 3.4 と CS-25 を同じ手番で書き替える。V は 1 行につき多くて 2 回（T と E0、または T と E）のままである。
   - **B（直列のまま、落ちる行を減らす）**：依頼の形は変えない。R8f の V の直し（branch `feat/r8f-verify`）を main に入れた後に、T の訳が V に落ちた行の理由を `hold_reason` ごとに数え、正しい訳を止めている理由（会話字幕の CS-24 の材料）だけを直す。ただし、V が正しく止めた行は E を待つままで、その行の `L` は縮まない。B で残る差の大きさは R8f の直しの後の run でしか分からない。
3. **確定の復号の待ちを測る**（RP-34）：復号の順は変えない。同じ run で 4〜7 秒の発話の `decode_ms` の p50 が Python の 183 ms に対し 275 ms、7 秒以上で 248 ms に対し 440 ms だった（原因の席の打ち直し。NOT VERIFIED）。ただし `decode_ms` は `decode_segment` の `timed` が `recognizer.decode` だけを測った値で、速報の復号を待った時間を含まない。したがって RP-34 は待ちを `L_asr − decode_ms` で測る。`on_window` は feed の後に速報の復号を 1 本回す直列で、確定より先に捨てられる速報の復号は無い（発話の終わりの検出の前提は ASSUMPTION）。挙動を変えない次の 3 つは、4 の run の前に入れる。
   - 接続を閉じた依頼の生成が 1 秒以内に止まるかの probe（R8e が `r8e_server.rs` に入れ、通った）。
   - window を受けてから feed を始めるまでの待ちの log（`hayamimi` の log の event）。
   - RP-32 の各 run の前後の `nvidia-smi --query-gpu=utilization.gpu,memory.used --format=csv,noheader,nounits` の記録。llama-server は Python の run の間も起こしたままにし、読みは各 run の直前の 1 回で比べる。Python と Rust の対で、直前の `utilization.gpu` のどちらかが 5 % を超えるか、直前の `memory.used` の差が 256 MiB を超える対は無効にする（5 % と 256 MiB は仮の値。256 MiB は RP-6 の幅に合わせた。持ち主は R8h で、最初の RP-32 の run の 6 本の直前の読みの幅を記録し、幅が 5 % か 256 MiB に近ければ評議へ出す）。

   採らない形は 2 つである。速報の復号を別の thread と別の session に移す形は、認識器を CPU に置くので（3.5）2 本の復号が core を取り合う。発話の終わりで始まっていない速報の復号を捨てる形は、捨てる対象が無い。
4. **同じ負荷で交互に測る**（RP-32）：Python と Rust を同じ機械で、Python、Rust、Python、Rust … の順に 3 対流し、各 run の前と後の `/proc/loadavg` と、中継の stdout と、`hayamimi` と Python の log を run の file と同じ所に残す。run の間は他の席が cargo の build を打たず、各 run の前の 1 分の load が 2 未満であることを記録する（R7 の測り直しと同じ規則）。合否は 3 対を合わせた標本で、RP-3 の期待値の式で決める。

**品質を下げない条件**（RP-35）：2 の B と、作る時の A のどちらでも、字幕と外の送り先に出る訳は全部 V を通った物で、V の検査（形、数、名前、否定）を緩めない。ja→en と en→ja の held-out（会話字幕の設計 9）の chrF と、V が止めた行の数が、R8h の前の main の値から下がらず増えない（同じ server、同じ model）。FLEURS の ja→en の chrF（RP-13 の en の行）も R8h の前の値以上である。3 の待ちの log は RP-1 の ja の CER と RP-5 の速報の遅れを悪くしない。

**R8f の V の直しとの関係**：2 の B は R8f の V の直しの上に乗り、A を採る時も、A の効き目は R8f の直しの後の run で測る。V の直しで T が V を通る行が増えれば、2 の対象の行は減るからである。R8h の 2 の席は `feat/r8f-verify` が main に入るまで、中継の `pipeline.rs` の V の呼び出しの所を書き替えない（同じ関数を 2 席が触ると衝突する）。

**R7 と R7b との関係**：R7 は認識の段の時間と置き場を持ち、R8h の 3 は確定の復号の前の待ちを測るだけで、復号の順を変えない。R7 の H10（確定の解き直しを省く）は同じ loop を触るので、H10 と R8h の 3 の待ちの log は同じ席が続けて打つ。R7b は日本語の streaming の認識器を測るだけで、`hayamimi` の経路を替えない（10 の H38）。streaming の認識器を採ると、速報は解き直しでなくなり、3 が測る待ちの形も変わる。採るかは E2E の後に評議が決める。

**順と置き場**：1 は W3 の空いた席がすぐ打ち、E2E-1 と E2E-2 も 1 の後の道具で測る。2 の B は R8f の V の直しの席が持つ。3 の待ちの log と GPU の記録は W3 の空いた席が 4 の前に入れる。2 の A は上の条件が揃った時だけ、E2E の後に作る。4 の run は E2E の波で打ち、E2E-1 の `L` の判定も 4 の run で決める（仮の置き場。持ち主は R8h で、E2E の波の 1 時間に 3 対が収まらなければ 2 対にし、その数を記録する）。

## 4. 採らない手段

- **Python を残して Rust から呼ぶ**：実行時の経路に Python が残り、置き換えにならない。
- **クラウドの認識と翻訳の API**：会話の音声と文字を外へ出す。
- **CPU の翻訳器の Rust 版（ct2rs で FuguMT と M2M-100）**：訳の質の上限が model にあり（`translate.md`、`translate_m2m.md` の失敗例）、GPU の言語モデルで今以上の chrF を測れる（RP-13）。
- **sherpa-onnx の配布の CUDA 版の archive をそのまま使う**：`sm_120` 系の判定（3.12）の材料になる機械語と PTX の件数と、ORT と CUDA の版を自分で固定できない。自分で build して `build.json` に記録する（3.12）。
- **CUDA 12.9.1 と cuDNN の CUDA 12 版を並べて入れ、sherpa-onnx の既定の CUDA 12 版の ORT を使う**：ORT 1.31.0 に CUDA 13 の公式の build が在り（退路の 1.28.3 にも在る）、環境変数 2 つで sherpa-onnx に渡せる。CUDA を 13.4 の 1 本にすれば、入れて保つ版が 1 つ減り、llama-server とも揃う。CUDA 13 版が動かないときの戻し先としてだけ残す（3.12）。
- **ORT を CUDA 13 で source から build し直す**：公式の CUDA 13 版の build が在り、手数を掛ける理由が無い。
- **llama.cpp を CUDA 12.4 の build にする**：12.4 は `sm_120` を持たず、認識の側の 13.4 と版が割れる（3.12）。
- **要る物が無いときに理由を出して起動を止める**：GPU、model、llama-server、送り先、port、装置のどれか 1 つが欠けただけで字幕が全部止まる。欠けた段だけを落として動かせる（3.11、3.15）。
- **黙って CPU や小さい model に落とす**：遅くなった理由も精度が下がった理由も利用者に見えず、GPU の在る機械で落ちても気付けない。落ちた段ごとに event と画面に出す（3.15）。
- **int8 の model を CUDA で動かす**：動的量子化の op が CUDA に無く、node が CPU に落ちる（3.4）。
- **形態素解析の crate（vibrato、lindera）を入れる**：句読点の token 化は「NFKC → 空白を除く → 1 文字ずつ」で MeCab 版と 102 件で一致している（`ja_pipeline.ja.md`）。
- **end-to-end の音声翻訳 model**：日英の両方向を、許諾と Rust の道の両方を満たして動かせる物が無い（3.6）。
- **評価の音声の選び方まで Rust に移す**：manifest に行を固定すれば足りる（3.2）。
- **学習と model の変換の道具を Rust に移す**：PyTorch に依存し、配布済みの model で足りる。
- **mobile/ を Dioxus に作り直す**：依頼の範囲の外で、wire 形式の互換で足りる。
- **`hayamimi-relay` を `hayamimi` の process に入れる**：会話字幕の設計の 2 音源を 1 か所で結ぶ形が変わる。1 段の SSE の時間は `L_tr` に入れて測る。
- **断片の訳の時刻を主指標に入れる**：原文が短いほど速く見え、`L` が負にもなる（3.1）。
- **V を緩めて T の先の訳を増やす**：`L` は縮むが、V が止めるはずの訳が字幕と外の送り先に出る。品質の条件（3.16、RP-35）に反する。
- **T が V に落ちた訳を E を待たずに出す**：V に落ちた訳は出さない（3.6 の 1）。相手の画面では置き換えられない。
- **E を外して T と V だけにする**：ja と en の間では E が直訳の型を減らす（会話字幕の CS-26）。外すと訳が硬くなり、外した時の chrF の差は測っていない。
- **別の日時の Python の記録と比べる**：負荷が揃わず、差が実装の差か機械の状態の差かを分けられない（3.16 の 4）。
- **`held` の空の訳を字幕が出たと数え続ける**：字幕に何も出ない時刻で `L` を測ることになり、V が止めるほど `L` が小さく出る（3.1）。

## 5. 構造

```
マイク / loopback（WASAPI）─┐
/ingest（WebSocket）────────┼─▶ f32、16 kHz（CPU）─▶ ring buffer、preroll（host）
wav ───────────────────────┘                                  │ const float*
                                                              ▼
            自分で build した CUDA 版の sherpa-onnx（provider は 3.5 の表のとおり、ORT 1.31.0 は CUDA 13 版）
            VAD ─▶ 言語判定 ─▶ 認識器（fbank、encoder、探索）─▶ 文字列
            CAM++ の埋め込み、pyannote の再分離
                                                              │
            ITN ─▶ 句読点（同じ build の ORT を ort で開く、CUDA、IoBinding）─▶ 置換辞書
                                                              │
            hayamimi::server（axum）─▶ /events、/ingest、/config ほか ─▶ hayamimi-ui（Dioxus）
                                                              │
            hayamimi-relay ─▶ T（llama-server、Windows、CUDA 13.4）─▶ V ─▶ 先の訳 ─▶ E ─▶ V ─▶ 確定の訳
                                                                                         └─▶ Zoom、Teams、SRT
```

GPU に置くのは言語判定、句読点の BERT、訳 T である。
認識（ReazonSpeech の transducer、int8）、VAD、話者の埋め込み、清書の再分離は、同じ精度の対で CPU の方が速いので CPU に置き、音声の前処理と文字列の段も CPU に置く（3.5）。

## 6. 受入 ID

`<hy>` は Windows では `native\target\x86_64-pc-windows-msvc\release\hayamimi.exe`、WSL では `native/target/release/hayamimi` を指す。
`<ev>` は同じ場所の `hayamimi-eval` を指す。
Windows の path は target の triple を含むので、`native/` で `cargo build --release --target x86_64-pc-windows-msvc` と `--target` を付けて build する。
`cargo test` は `native/` で打つ。
`<base>` は R0 が書く基準の file（`testdata/baseline/`、git 管理外。要約は `docs/results/baseline_python.md`）で、Python の実測である。
R0 の受入（RB-1〜RB-8）は `rust_port_tests.ja.md` に在る。
目標値のうち「Python の値以下」と書いた物は R0 の実測に結びつく。それ以外の数のうち、対の比較の 20 / 23 と根拠 3 の 1% は R0 と R1 の実測で裏づけた値である。2000 ms、20%、14,000 MiB は実測の裏づけが無いまま据え置いた値で、持ち主の slice が測って超えたら評議へ戻す（10 の H8）。

| ID | 測る物 | command | 期待値 | 陽性対照 | 陰性対照 |
|---|---|---|---|---|---|
| RP-1 | 認識の誤り率（scorecard と同じ組と採点） | `<ev> scorecard --engine hayamimi --sets testdata/eval_real,testdata/eval_real_zhko,testdata/eval_real_yue --out rp01.json` の後に `cargo test --release --test rp01 -- --ignored` | 言語ごとの平均の誤り率が、R0 の 2 回の Python の出力（`<base>/scorecard_run1.md` と `scorecard_run2.md`、RB-3）の言語ごとの平均以下（ja、zh、ko、yue は CER、en は WER）。Python の値は小数 3 桁に丸めた記録しか無いので、Rust の平均も小数 3 桁に丸めて（四捨五入）から比べる。測るのは配る形の build（自分で build した sherpa-onnx と 3.12 の ORT）で、Python の wheel の ORT を読ませた run は、Rust の code が Python と同じ物を出すことの確かめ（clip ごとの値が Python の log と小数 3 桁で一致）にだけ使い、合否に使わない。言語判定は `hayamimi-asr` の振り分けで決め（clip の正解の言語を渡さない）、LID の正解の数を数える。yue は Python と同じく OpenCC の t2s で正規化してから採点し、Rust の側は `ferrous-opencc`（0.4.0、Apache-2.0、OpenCC の Rust 実装）の t2s を使う（10 の H24）。LID の正解の数も Python 以上。値は `docs/results/scorecard.md`（ja 0.038、en 0.023、zh 0.066、ko 0.081、yue 0.061）と並べて記録する。Windows の配る形の build で振り分けを通した run では、zh と yue は Python と同じ値である（10 の H35。NOT VERIFIED で、検証の席が可を返したら外す） | `<base>` の Python の値を同じ test が読み、scorecard.md と 0.005 以内で一致する | 振り分けで ja の clip を SenseVoice の tier へ送る build では、ja の CER と ja の LID の正解の数が Python より悪くなり落ちる（scorecard の経路は句読点の段を通らず、採点も句読点を捨てるので、句読点を外した build は通常の run と同じになり、対照にならない） |
| RP-2 | ja の golden 8 本 | `cargo test --release --test rp02 -- --ignored` | 各 clip の確定文の FLEURS の参照文に対する CER が、`golden.json` の `cer_finals_vs_reference` 以下（清書の `cer_refine_vs_reference` は RP-19 が写す `test_ja_golden.py` の case が見る）。GPU で出た文字列は `tests/golden/ja/golden_gpu.json` に別に記録する | 8 本の参照文と記録 CER を読めること | ITN を外した build で `ja_085` と `ja_090` が落ちる |
| RP-3 | 主指標 `L`（発話の終わり → 原文が `final` と一致した訳の字幕） | Python と Rust を同じ機械で順に起こし、`<ev> stream --wav lat-ja.wav --repeat 5 --out <run>.jsonl` を各 1 回、`cargo test --release --test rp03 -- --ignored` が 2 本を読む。R0 で決めた `--min-silence` の値を両方に渡す | ja→en：Rust の `L` の p50 と p95 が、Python（`--translate en`、FuguMT）の p50 と p95 以下。`L` の比較集合（3.1。R0 の Python の 2 回では、発話 5、10、18、19 を除く 19 発話）のうち 17 発話以上で、Rust の 5 回の中央値が Python の 5 回の中央値以下（17 は比較集合の発話の数 × 20 / 23 の切り上げ。Python 同士の 2 回の対では 19 発話中 10 で、17 以上が偶然で通る確率は独立を仮定して 191 / 2^19 = 0.036%。`baseline_python.md:171-195`）。en→ja（lat-en）：p95 が 2000 ms 以下（lat-en の Python の基準が無いまま据え置いた値。R8e が測り、超えたら評議へ戻す。10 の H8）。`L_asr` と `L_tr` を別の列で出し、`L_tr` が負の発話の数を出す | 同じ test が Python の run から p50 と p95 を出せ、各 run で 有効 + final なし + 終端不正 が枠の 23 × 5 と一致し、有効な標本の全件で `L_asr − L_dec ≥ min_silence` が成り立つ（3.1 の分割式） | 訳の依頼に 600 ms の待ちを入れた偽の llama-server で落ちる。原文が `final` と一致しない訳だけを返す偽の中継では `t_tr` が取れず落ちる |
| RP-4 | 認識の遅れ `L_asr` | RP-3 の同じ 2 本 | `L_asr` の p50 と p95 が Python 以下。`L_asr` の比較集合（3.1）で、Rust の 5 回の中央値が Python 以下の発話が、比較集合の発話の数 × 20 / 23 の切り上げ以上（RP-3 と同じ下限）。`L_dec`（各実装の log の値）は記録だけ | 同じ run の全 `final` の `latency_ms` の平均と本数を、WS の `final` event と Python の stdout の log の 2 つから別々に出し、本数が同じで平均の差が 0.5 ms 以内（WS の `latency_ms` は丸めない値で、Python の stdout の log は `latency=` を整数 ms に丸めて出す。1 本の丸めの誤差は 0.5 ms 以内なので、平均の差も 0.5 ms を超えない）。発話の最後の `final` だけの平均とは母集団が違うので比べない | `provider` を `cpu` にした Rust の run の値を同じ式で出し、記録する |
| RP-5 | 速報の字幕と速報の訳 | RP-3 の同じ 2 本 | `t_partial − ` 発話の始まりの sample を送った時刻、の p95 が Python 以下。Python の基準は Rust の run と同じ `--min-silence`（比較の値 0.30、10 の H9）で流した run とし、RP-32 の Python の run が在ればそれを、無ければ `testdata/baseline/lat_ja_0.30_1` を使う（0.35 の記録の run は使わない）。速報の訳が出た発話では、その時刻が `t_tr` より前 | Python の run から、`final` を持つ全発話で最初の `partial` の時刻が取れる。`final` の無い発話（3.1 の「final なし」）は、`partial` が在っても無くても p95 に入れず、p95 は Python と Rust の両方を `final` を持つ発話で取る。`partial` だけが在って `final` の無い発話の数と、どちらも無い発話の数は、結果の file に別々に書く（記録した run では 115 行中 12 行がどちらも無い発話）。`partial` だけの発話を除くのは、`L` の比較集合（3.1）と同じ発話の組で RP-5 を読むためである | 速報の訳を切った設定で、速報の訳の数が 0 |
| RP-6 | 推論が GPU で走ったこと（provider の log と nvidia-smi） | ほかの GPU の process（llama-server ほか）を止めた機械で、`nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits` を起動の前に 3 回、`<hy> --wav tests/golden/ja/ja_014.wav --no-realtime --mode single --lang ja --gpu-diagnose` の実行中と終了後 10 秒まで 0.25 秒以下の間隔で止めずに読み続け（GPU の memory の山は 0.5 秒ほどで、1 秒ごとでは取り逃す）、`cargo test --release --test rp06 -- --ignored` が log と読みを読む。GPU の memory を読む rp06 の run は同時に走らせない。`--query-compute-apps` は使わない（WSL では process の名前が `[Not Found]`、Windows では使用量が `[N/A]` になり、どちらの側でも名前と使用量の組が読めない） | 起動前の 3 回の中央値を基準として、実行中の最大が基準より 256 MiB 以上多く、終了後 10 秒以内に基準 ± 64 MiB へ戻る（256 MiB と 64 MiB は仮の値。Windows の実測では基準 1198 MiB、実行中の最大 1957 MiB、終了後 1198 MiB。10 の H8c）。3.5 の表で GPU とした段の `gpu.session.created` の全件で実際の provider が `cuda`、CPU とした段は `cpu`。`gpu.session.placed` で CPU の node の数を model ごとに記録し、3.5 の表で GPU とした model は、`gpu_stages.md` の node の表（「CUDA の node」と「CPU の node」の表）の R1 の値以下（句読点の BERT は 77）。GPU とした model が node の表に行を持たなければ落ちる。3.5 の表で CPU とした model と、表に載らない model は記録だけにする | 同じ読み方で、llama-server を起こすと `memory.used` が基準より 256 MiB 以上増え、止めると戻る | `CUDA_VISIBLE_DEVICES` を空にして起こした run では、実行中の最大が基準 + 64 MiB を超えず、`gpu.device.unavailable` が 1 件 |
| RP-7 | 段の間の転送 | R1 の計測（RP-23）の中で `nsys profile --stats=true <hy> --wav ...` を打ち、`cargo test --release --test rp07 -- --ignored` が統計を読む | 1 発話あたりの host → device と device → host の回数、bytes、時間の p50 と p95 が `docs/results/gpu_stages.md` に在る。転送の時間の p95 が認識の時間（`decode_ms`）の p95 の 20% 以下（据え置いた値。R1 の 1.37% は認識を CUDA に置いた構成の値で、CPU に置いた構成の値ではない。R7 が測り、超えたら評議へ戻す） | 同じ式で encoder の kernel の時間も出る | int8 の model を CUDA で読ませた run では転送の回数が増える |
| RP-8 | CPU に残した段の根拠 | `cargo test --release --test rp08` が `docs/results/gpu_stages.md` と 3.5 の表を突き合わせる（test は `#[ignore]` を持たない。docs だけを読み、testdata も GPU も要らない） | 3.5 の表で CPU とした段の全行が、3.5 の根拠 1〜4 のどれかを持つ。根拠 1 は、根拠の列の「根拠 1（variant `<名>`）」が名指しした 1 つの variant だけについて、`gpu_stages.md` の GPU と CPU の行の対で CPU の p50 が GPU の p50 より小さい。同じ段のほかの variant の対は、CPU と GPU のどちらが速くても判定に使わない。variant を混ぜた最小値で比べない。名指しが無い、2 つ以上在る、名指しした variant の対が欠けている、のどれも落ちる。根拠 2 は GPU の `transfer-floor` の行と CPU の行の対で CPU の p50 が小さい。根拠 3 は CPU の p50 が `L` の見積りの p50 の 1% より小さい。根拠 4 は、`gpu_stages.md` の CPU の行が未測定の間は根拠の列に持ち主の slice（R2、R4、R5）の名前と GPU の経路が無い理由（取り込みは一次資料の URL）が在り、測った後は CPU の p50 が GPU の往復の下限（3.5）より小さい | GPU に置いた段の行も、GPU と CPU の p50 と p95 を同じ形で持つ | 根拠の列を空にした表で落ちる。根拠 4 の行の CPU の p50 を GPU の往復の下限と同じ値にした結果で落ちる。CPU の int8 の行だけが GPU の fp32 より速く、同じ fp32 の対では GPU が速い表で落ちる |
| RP-9 | 実行時の Python が 0 本 | Windows で `<hy> --input mix --serve` と `hayamimi-relay live` を 10 分動かし、1 秒ごとに `Get-CimInstance Win32_Process` で `hayamimi` の子孫の process の名前を集める。`dumpbin /dependents <hy>` の DLL の名前。R11 の後は `git ls-files '*.py' \| wc -l` | 子孫に `python` を含む名前が 0。DLL に `python` を含む名前が 0。`*.py` が 0 本 | R0 の Python の run では同じ式で `python.exe` が 1 以上 | 子 process に Python を立てる偽の build で落ちる |
| RP-10 | Windows で動く | Windows 11 で `cargo test --release --test rp10 -- --ignored`（loopback へ英語の wav を鳴らし、`/events` を読む） | `ready` の後に `final` が 1 件以上、`lang` が `en`、`partial` が 1 件以上。マイクは入力の装置が在るときだけ同じ形（会話字幕の CS-23 と同じ前提） | 同じ test を WSL で `--wav` で打つと通る | 出力の装置を止めた run では止まらず、`capture.fallback.single` が 1 件出てマイクだけで `final` が出る（RP-29 の行） |
| RP-11 | WSL で動く | WSL で `cargo test --release --test rp11 -- --ignored`（`--wav` と `/ingest`） | 両方の入力で `final` が出て、`gpu.session.created` の provider が 3.5 の表の置き場と同じ（GPU とした段は `cuda`） | 同上 | `/ingest` に 8 kHz の宣言で 16 kHz を送ると、resample の後の CER が落ちて test が落ちる |
| RP-12 | wire 形式の互換 | R0 で Python の server から録った event と応答（`<base>/wire/`）を、`cargo test --release --test rp12` が Rust の出力と比べる | R0 で録れた event の型ごとに key の集合と値の型が同じ（Rust が足す 3.3 の 5 項目と `src_sha256` と、3.15 の `fallback` の event の型と、R4 の `speaker` の event の型は差から除く）。8 本の route と `/ingest` の handshake が同じ status と key を返す。mobile の `remote_event_test.dart` の fixture を Rust の出力で置き換えても `dart test` が通る | Python の録りを Python の録りと比べて差 0 | key を 1 つ消した event で落ちる |
| RP-13 | 訳の品質 | `<ev> fleurs-pairs --targets en,zh,ko,es --n 50 --seed 0 --out pairs/` の後に `hayamimi-relay eval --pairs pairs/ --out hyps/`（中継の `eval` は slice 1 に無く、R8f で足す。llama-server が要る）、`<ev> chrf --pairs pairs/ --hyps hyps/` | 確定の訳の corpus chrF が en 47.57、zh 20.19、ko 25.20、es 42.09 を超える（en は会話字幕の CS-3 と同じ）。V が両方の訳を止めた行は空の hyp で数える。V の直しの効果は、FLEURS の test split から新しく引いた組（`fleurs-pairs --split test`）を 1 回だけ使って出し、その組は直す席に渡さない。RP-13 は en 以外も含め未通過のまま記録し、R11 の入口に残す（10 の H36） | 参照訳を hyp に渡すと 100 | 原文を返した hyp では値を割る |
| RP-14 | Rust の chrF が sacrebleu と同じ値を出す | R0 で sacrebleu 2.6.0 が採点した組（`<base>/chrf_fixture.tsv` と値）を `cargo test --release --test rp14` が読む | 差の絶対値が 0.01 以下 | 同じ fixture の文 1 本 | n-gram の上限を 5 にした実装で落ちる |
| RP-15 | 話者の分離 | `<ev> der --set testdata/eval_diar --collar 0.25` の後に `cargo test --release --test rp15 -- --ignored`。確定文が話者を待たないこと（T-RP-15-012、013）は `cargo test --release -p hayamimi --test rp15_speaker -- --ignored` | AMI 5 会議の平均 DER から `<base>` の Python の 3 回以上の run の平均を引いた差が、Python の回の間の幅（平均 DER の最大 − 最小）より小さい（Python の平均以下なら差は 0 以下で通る。差が幅と等しいか大きければ落ちる）。差と幅を同じ結果の file に書く。幅で読むのは、Python の pyannote と FastClustering の run ごとの揺れの中では、Rust の値が Python より悪いとも良いとも言えないからである | `<base>` に Python の `--method refine_diarize` の run が 3 回以上在り、同じ test がその平均と幅を出せる（2 回では幅を決めない）。`diarization.md` の値は入力の音声の sha256 が残っていない記録なので比べない | 清書の再分離を切った run では落ちる |
| RP-16 | GPU の無い機械では段ごとに CPU へ落として字幕を出す | `CUDA_VISIBLE_DEVICES` を空にして `<hy> --wav tests/golden/ja/ja_014.wav --no-realtime --mode single --lang ja` を起こし、同じ run の `/events` を読む `cargo test --release --test rp16 -- --ignored` | 終了 code が 0、`final` が 1 件以上、`gpu.device.unavailable` が 1 件（`reason` が `no_device`）。その run が読み込む 3.5 の GPU の段のそれぞれに `gpu.fallback.cpu` が 1 件ずつ（R2 の ja の run では句読点の 1 件、R3 以降は言語判定も）、同じ段の `/events` の `fallback`（`to` が `cpu`）が 1 件ずつ。`gpu.session.created` の実際の provider が全件 `cpu`。stderr に `nvidia-smi` の名前が在り、`LANG=ja_JP.UTF-8` と `LANG=C.UTF-8` で stderr の文が別の言語 | GPU の在る run では `gpu.device.unavailable` と `gpu.fallback.cpu` と `fallback` の event が 0 件で、GPU の段の provider が `cuda`（GPU が在るのに CPU へ落ちない） | GPU が在るのに CPU へ落ちる build（session を `cpu` で作る build）では、GPU の在る run で `gpu.fallback.cpu` か `cpu` の provider が出て落ちる。落ちたのに event を出さない build では、GPU を隠した run で event の数が足りず落ちる |
| RP-17 | 自分で build した library が開け、ORT が 1 つである（R1 の門） | Windows と WSL で `hayamimi-gpu-probe`（`hayamimi-gpu` crate の bin。`native/target/<triple>/release/` に在る）を走らせ、`cargo test --release --test rp17` が子の process の出力を読む（`rp17.rs` の test は `#[ignore]` を持たないので `--ignored` を付けない。付けると 0 本しか走らない）。Linux 限定の test（T-RP-17-031）は WSL から打つ。`hayamimi` crate は R2 で作るので、`<hy> gpu-probe` はそれまで無い | `cuda_devices: 1`、sherpa-onnx の CUDA の session が 1 本作れて実際の provider が `cuda`、`ort` が開いた ORT の `GetVersionString` が `build.json` の ORT の版（1.31.0。退路へ戻したときは 1.28.3）と同じ、process の中の `onnxruntime` の library が 1 つで、その読み込み元が実行 file と同じ directory（probe の `modules` の origin が `exe_directory`）。CUDA の版は、認識器が開いている間の process の読込 module の名前で見る（`build.json` の申告ではない。名前は大文字と小文字を区別せずに数える）。Windows は `cublas64_13` と `cublasLt64_13` が各 1、`cudart64_12` と `cublas64_12` と `cublasLt64_12` が 0。`cufft64_12` は在ってよい（CUDA 13.4 の toolkit 自身が `cufft64_12.dll` を持つので、`*_12` を一律 0 にはしない）。WSL は `libcudart.so.12` が 0、`libcublas.so.13` が 1。ORT の CUDA の provider の library の中の文字列では CUDA の版を判別しない（`cudart64_12.dll` と `cudart64_13.dll` の文字列を両方持つ）。Windows では、`hayamimi-gpu` を依存に持つ全 crate の bins と tests に 3.4 の 2 本の link 引数が在る（`cargo metadata` で依存する crate を数え、各 crate の build script を読む）。通ったときは、その build の `build.json` の sha256 と probe の出力を `native/target/sherpa-gpu/<os>/rp17_passed.json` に書く（RP-24 が読む） | 2 つの OS で同じ出力の形 | 配布の CPU 版の sherpa-onnx に繋いだ build では provider が `cpu` で落ちる。`onnxruntime` の読み込み元が `system` の run で落ちる |
| RP-18 | 句読点の同一性 | `cargo test --release --test rp18` が `mobile/hayamimi_core/test/fixtures/punct_ja_parity.json` を読む | 全 case で `input_ids` が一致。GPU で `expected` の文字列が全件一致 | fixture の 1 件目 | NFKC を外した token 化で落ちる |
| RP-19 | Python の test の case を Rust へ写したこと | `cargo test --release -- --list` の出力と、test-designer が `rust_port_tests.ja.md` に書く対応表を `cargo test --release --test rp19` が突き合わせる | 対応表の全 386 行が、Rust の test の名前か `写さない：<理由>` のどちらかを持つ。名前を持つ 334 行は list に在り、全部通る。`写さない` を許すのは 3 種だけで、3.2 が消す script の test、この設計で無くなる部品（Python の中の翻訳 worker と翻訳器の pool、文ごとの訳の分割。訳は `hayamimi-relay` が 1 行ずつ頼む、3.6）、外が更新する model の sha256 を動作の gate にする case（sha256 は `models/manifest.json` の記録に留める、3.10）である。`docs/spec/ja_pipeline_spec.json` の `config` を `<hy> config dump` で作り直した値と差 0 | 対応表の 1 行目 | 名前を 1 つ変えた list で落ちる |
| RP-20 | 3.5 の表の全段の置き場が決まったこと | `cargo test --release --test rp20` が 3.5 の表の「いまの置き場」の列に「測る」が残っていないことと、各行が `gpu_stages.md` の行に対応することを確かめる（test は `#[ignore]` を持たない。docs だけを読む） | 「測る」が 0 件、表の行と結果の行の対応が全件 | R1 の前の表（`native/hayamimi-eval/tests/fixtures/rp20_table_before_r1.md`、「いまの置き場」の列に「測る」が 8 件）を陽性対照の入力にし、同じ式が 8 件と数える | 結果の file の行を 1 つ消すと落ちる |
| RP-21 | model の取得に Python が要らない | 空の directory で `<hy> models fetch --minimal --dir <tmp>` | 今の `--minimal` と同じ 4 組（ReazonSpeech、whisper-tiny、Silero、句読点）が 3.5 の置き場に合った版（3.10）で揃い、`models/manifest.json` に sha256 が在る | 既に在る directory で打つと取り直さない | 取得先の 1 つを壊すと、もう 1 つの取得先（sherpa-onnx の release と Hugging Face）から取る。両方を壊すと前の file を残し、`models.fallback.previous` を 1 件出して終了 code 0 |
| RP-22 | GPU の memory の上限 | RP-3 の run の間、`nvidia-smi --query-gpu=memory.used --format=csv` を 1 秒ごとに読む（R3b は `/ingest` が入る前なので、llama-server を起こした機械で `<hy> --wav testdata/lat-multi.wav` を実時間で流す間に読む。8 の「07:00 の計画」の R3 の着手の 2） | 最大が 14,000 MiB 以下（`gpu_stages.md` に memory の実測が無いまま据え置いた値で、R3 と R8e が測り、超えたら評議へ戻す。16,311 MiB から、他の process の 1,161 MiB と、余白の約 1,150 MiB を除いた値） | 起動前の値が読める | 上限を、その run の起動前の読みの最大より 1 MiB 高い値に置いた test の設定（読む物は GPU 全体で、他の process の使用量は run ごとに動くので、別の run の最大から置いた上限は、その run が届かない所に在りうる。起動前の読みから置いた上限は、読み込んだ後の `hayamimi` の分だけ必ず下回る）では、`hayamimi` が止まらず `gpu.fallback.cpu`（`memory_budget`）を 1 件以上出し、test の比べる式は上限を超えた読みを名指しして落ちる（比べる式が働くことの確かめ） |
| RP-23 | 段ごとの GPU と CPU の計測（R1） | `<ev> stages --provider cuda --wav lat-ja.wav --out stages_cuda.jsonl` と `--provider cpu` を同じ build で 1 回ずつ（1 stream）打ち、同じ 2 本を llama-server に訳の依頼を流し続けた負荷の下でも打ち、`cargo test --release --test rp23 -- --ignored` が `docs/results/gpu_stages.md` を書いて読む | 3.5 の表で GPU の経路が在る段（「いまの置き場」が「CPU（GPU 経路なし）」で始まらない 14 段）について、無負荷と負荷の下の、GPU と CPU の p50 と p95（標本は lat-ja の 23 発話を 5 回流した区切りの数か、文字列の段は同じ数の行）、1 発話あたりの転送の時間と bytes、測った機械と `build.json` の要点が在る。JIT の cache が空の run と温かい run の `decode_ms` が別の行で在る（10 の H27） | 同じ build で同じ入力を 2 回流した p50 の差が 10% 以内 | provider を指定しない run の行は、CPU の行と同じ値になり、GPU の行として受け付けない |
| RP-24 | build の再現（Windows と WSL） | 2 つの OS で `cargo run -p xtask -- sherpa-gpu` を打ち、`cargo test --release --test rp24 -- --ignored` が 2 つの `build.json` を比べる。2 つの OS を比べる `windows_and_wsl_builds_match` は WSL から打つ（WSL の build の directory は symlink で、Windows からは WSL が作った symlink を読めない。os error 1920） | sherpa-onnx の commit、ORT の版、CUDA の主版、cuDNN の主版、`sm_120` 系の判定（3.12）が一致。各 OS で、`sm_120` か `sm_120a` の機械語が 1 件以上、または `sm_120` の PTX があり「PTX の JIT で RP-17 が通った記録」が在る。この記録は、その build の `build.json` の sha256 を持つ `rp17_passed.json`（RP-17）である | 同じ OS で 2 回 build した `build.json` の上の項目が一致 | ORT の版を変えた build では差を名指しして落ちる |
| RP-25 | 訳の揺れ | RP-3 の Rust の run を `cargo test --release --test rp25 -- --ignored` が読む | 発話ごとに、確定の訳が出た後に表示の訳が変わった回数が 0。先の訳から確定の訳への置き換えは 1 回まで。速報の訳で、確定した字形で描いた接頭辞が後で消えた回数が 0。Zoom と Teams と SRT へ出た訳は 1 つの `id` につき 1 件で、確定の訳と同じ | 先の訳と確定の訳が違う発話の数を同じ test が数える | 確定の訳の前に先の訳を送る偽の中継で落ちる |
| RP-26 | 言語の切り替え | `cargo test --release --test rp26` が、`<hy> --help` と、fallback の時の stderr（3.15）を、locale を en と ja にして読む | en と ja で文が別の言語で、どちらも `strings.<locale>.toml` の key だけから組まれている。`strings.en.toml` と `strings.ja.toml` の key の集合が同じ | en の出力が英語 | 文言を code に直書きした build で、表に無い文が出て落ちる |
| RP-27 | GPU の経路が無い文字列の段が ORT と CUDA を呼ばない（R2） | `cargo test --release --test rp27` が `rg -c -i '\bort\b\|cuda' native/hayamimi-asr/src/itn.rs native/hayamimi-asr/src/punct/rules.rs` を子の process で打つ | 2 つの file が在り、当たりが 0 件（rc 1）。file が無いときの rc 2 は落ちる | 同じ式を `native/hayamimi-gpu/src/` に掛けると 1 件以上（rc 0） | `itn.rs` に `use ort` の 1 行を足した fixture の木で落ちる |
| RP-28 | CPU に落ちた時の主指標 `L` | RP-3 と同じ `<ev> stream --wav lat-ja.wav --repeat 5` を、`CUDA_VISIBLE_DEVICES` を空にした Rust の run で 1 回流し、`cargo test --release --test rp28 -- --ignored` が読む | GPU の run の合否（RP-3）とは別の行に、CPU に落ちた run の `L`、`L_asr`、`L_tr` の p50 と p95 を出す。仮の上限は `L` の p50 が 4500 ms、p95 が 7000 ms（R1 の「Rust 認識 cpu / 訳 cpu 無負荷」の見積り 4053 ms と 6194 ms、`gpu_stages.md` の「遅れ L の見積り」の同じ名の `L` の見積りの行、:304 に約 1 割を足した値。10 の H8）。R8e が測って置き直す | 同じ test が GPU の run からも同じ列を出せる | GPU の在る run を CPU の run として渡すと、`gpu.fallback.cpu` が 0 件なので CPU の run として受け付けない |
| RP-29 | GPU 以外の資源が無くても止まらない（3.15 の表の場面） | 3.15 の表の場面ごとに、資源を外した run と在る run を対で起こす `cargo test --release --test rp29 -- --ignored`（model の file を外した directory、llama-server を起こさない、送り先を閉じた port、8833 を先に塞いだ port、出力の装置を止めた機械、sherpa-onnx の library を外した directory、壊した設定の file）。各行は持ち主の slice が足す | 資源を外した run は、終了 code が 0、`final`（推論の library を外した行では server の `ready`）が出て、その場面の `<domain>.fallback.<落ち先>` の log の event と `/events` の `fallback` が段ごとに 1 件ずつ出る | 資源の在る run では、同じ場面の fallback の event が 0 件（在るのに落ちない） | 資源を外したのに event を出さない build で落ちる。資源が在るのに落ちる build（常に小さい model を読む build など）で、在る run の event が 1 件以上になり落ちる |
| RP-30 | `fallback` の `stage` と `reason` の値の一覧が code と同じ | `cargo test --release -p hayamimi --test rp30` が、3.15 の「`fallback` の event の `stage` の値は」の段落の 2 つの一覧と、`native/hayamimi/src/notice.rs` の `STAGES` と `REASONS` を突き合わせる（test は `#[ignore]` を持たない。docs と code の定数だけを読む） | `hayamimi` の `stage` の 13 個と `reason` の 11 個が、順を問わず集合として code と一致する。どちらかにだけ在る値は名指しして落ちる | 同じ test が、3.15 の段落から `stage` を 13 個、`reason` を 11 個読める | 一覧から 1 つ消した docs の fixture、code に無い値を 1 つ足した docs の fixture のどちらでも、その値を名指しして落ちる |
| RP-31 | `L` の数え方（3.1 の `t_tr` の条件と結び方、3.16 の 1） | `cargo test --release -p hayamimi-eval --test rp31` が、`stream` の集計の関数に fixture の event の列を渡す（`#[ignore]` を持たない） | `status` が `held` か `unavailable` の訳と `text` が空の訳は `t_tr` に入らず、同じ `id` の後の `ok` の訳が `t_tr` になる。Rust の run では `final` が `t1_s` で発話に結ばれ、次の発話の窓の中に着いた `final` も自分の発話の標本に入る。Python の run では、前の窓が「final なし」で次の窓に `final` が 2 件以上在る標本の数が別の列に出る | 同じ関数が、`held` の無い fixture で今の数え方と同じ `L` を出す | `held` の空の訳を先に置いた fixture で、空の訳の時刻を `t_tr` にする数え方を落とす。遅れて着いた `final` を着いた時刻で割る数え方を、その発話が「final なし」になることで落とす |
| RP-32 | 同じ負荷で交互に測った `L`（主指標、3.16 の 4） | 3.16 の 4 の順で `<ev> stream --wav lat-ja.wav --repeat 5 --out <run>.jsonl` を Python と Rust で交互に 3 対流し、`cargo test --release --test rp32 -- --ignored` が全 run と各 run の `/proc/loadavg` の記録を読む | 各 run の前の 1 分の load が 2 未満で、順が Python、Rust の交互。3 対を合わせた標本で、Rust の `L` の p50 と p95 が Python の p50 と p95 以下（RP-31 の数え方）。RP-3 の対の比較（17 発話以上）も同じ標本で数える。Rust の有効な標本のうち訳の取れた割合が 20 / 23 以上で（E2E-1 と同じ値）、分母は発話の標本（manifest の発話 × `--repeat` の回。1 つの run で 23 × 5）のうち 3.1 の「有効」に入る物の数とし、`final` の数や訳の依頼の数では数えない。`held` だけの標本と、`t1_s` で自分の発話に結んだ遅れた `final` の標本は、有効なら分母に入り、`t_tr` が取れなければ分子に入らない。有効の判定（3.1 の分割式）は RP-3 の test から `hayamimi-eval` の集計の関数へ移し、RP-3 と RP-32 が同じ関数を使う。Python の同じ割合からの欠けも別の列に出す。訳の取れない標本は p50 と p95 から外れ、V が止めるほど `L` が小さく出るので、割合を合否に入れる。`L_asr` と `L_tr` と、「訳」の段（`final` から `t_tr` まで）の p50 と p95 を別の列に出す | 同じ test が Python 同士の 2 run の対から同じ列を出せる | load の記録が無い run、1 分の load が 2 以上の run、Python を続けて 2 回流した順のどれかが在れば、値を出さずに落ちる。訳の取れた割合が 20 / 23 未満の Rust の run（R8e の記録の 67 / 93 の形）は、`L` が Python 以下でも落ちる |
| RP-33 | T が V に落ちた行の最初の訳（3.16 の 2） | RP-32 の Rust の run を `cargo test --release --test rp33 -- --ignored` が読み、中継の stdout の行ごとの記録で、T の訳が V に落ちた行と通った行に分ける | 「訳」の段の p50 と p95 が、全行で Python の「訳」の段の p50 と p95 以下。T が V に落ちた行だけの p50 と p95 と行の数を別の列に出す。B（R8f の V の直し）の後の run で打つ。A を作った時は、T が V を通った行の `L` の p50 と p95 が A の前の run 以下であることも足す | 同じ test が、T が V を通った行だけの列も出せる | T が V に落ちた行の最初の訳を 600 ms 遅らせる偽の中継で落ちる |
| RP-34 | 確定の復号の前の待ち（3.16 の 3） | RP-32 の Rust と Python の run を `cargo test --release --test rp34 -- --ignored` が読み、`final` ごとに `L_asr − decode_ms` を出し、発話の長さ（4 秒未満、4〜7 秒、7 秒以上）で分ける。Rust の run は window を受けてから feed を始めるまでの待ちの log と、各 run の前後の `nvidia-smi` の `utilization.gpu` と `memory.used` の記録も読む | 3 つの長さの組のそれぞれで、Rust の `L_asr − decode_ms` の p50 が Python 以下。`decode_ms` の p50 と待ちの log の p50 は記録の列で、合否に使わない。GPU の負荷の記録が Python と Rust の対で違う対は無効にし、その数を出す（Python の `decode_ms` の意味は確かめていない。10 の H43）。速報は RP-5 を満たす | 同じ test が、速報を切った Rust の run からも同じ列を出せる | 確定の復号の前に 300 ms の待ちを入れた偽の engine の run で、3 つの組のどれかが落ちる。GPU の記録が無い run で、値を出さずに落ちる |
| RP-35 | `L` の詰めで品質を下げない（3.16 の品質の条件。2 の B を入れた時と、A を作った時の両方で打つ） | R8h の前の main と R8h の branch で、同じ llama-server と model に `<relay> eval --final --heldout <path> --direction ja-en` と `--direction en-ja`、RP-13 の en の行を打ち、`cargo test --release --test rp35 -- --ignored` が 2 組の結果と RP-32 の Rust の run の中継の stdout を読む | held-out の chrF が両方の向きで前の main 以上、V が止めた行の数が前の main 以下（止める行が増えない）、RP-13 の en の chrF が前の main 以上。RP-32 の run で、`status` が `ok` の訳の全件に、その訳の文字列に掛けた V の `ok` の記録が在る。3 を入れた時は RP-1 の ja の CER が前の main 以下 | 同じ test が前の main 同士の 2 組から差 0 を出す | E0（または E）の応答に V を掛けずに出す偽の中継で、V の記録の無い `ok` の訳が 1 件以上になり落ちる。T の訳をそのまま確定にする偽の中継で held-out の chrF が下がり落ちる |

## 7. test の名前

各 ID の下の case 行（`T-RP-<n>-nnn`）は test-designer が `rust_port_tests.ja.md` に書く。

| ID | 受入 test の名前 | 置き場 | 陰性の case |
|---|---|---|---|
| RP-1 | `rp01_scorecard_error_not_worse_than_python_baseline` | `native/hayamimi-eval/tests/rp01.rs` | ja の clip を SenseVoice の tier へ送る build |
| RP-2 | `rp02_ja_golden_cer_not_worse_than_recorded` | `native/hayamimi-asr/tests/rp02.rs` | ITN を外した build |
| RP-3 | `rp03_speech_end_to_translation_beats_python` | `native/hayamimi-eval/tests/rp03.rs` | 600 ms 遅い偽の llama-server、断片の訳だけを返す偽の中継 |
| RP-4 | `rp04_recognition_latency_beats_python` | `native/hayamimi-eval/tests/rp04.rs` | provider が cpu の run（記録） |
| RP-5 | `rp05_partial_and_partial_translation_arrive_early` | `native/hayamimi-eval/tests/rp05.rs` | 速報の訳を切った設定 |
| RP-6 | `rp06_inference_runs_on_cuda` | `native/hayamimi/tests/rp06.rs` | GPU を隠した run |
| RP-7 | `rp07_transfers_between_stages_are_measured_and_small` | `native/hayamimi-eval/tests/rp07.rs` | int8 の model を CUDA で読ませた run |
| RP-8 | `rp08_every_cpu_stage_has_its_evidence` | `native/hayamimi-eval/tests/rp08.rs` | 根拠の空いた表 |
| RP-9 | `rp09_no_python_at_run_time` | `native/hayamimi/tests/rp09.rs` | Python を子に立てる偽の build |
| RP-10 | `rp10_windows_loopback_reaches_events` | `native/hayamimi/tests/rp10.rs` | 出力の装置の停止（止まらずマイクだけで動く） |
| RP-11 | `rp11_wsl_wav_and_ingest_reach_events_on_cuda` | `native/hayamimi/tests/rp11.rs` | 宣言と違う sample rate |
| RP-12 | `rp12_wire_format_matches_python` | `native/hayamimi/tests/rp12.rs` | key を消した event |
| RP-13 | `rp13_translation_chrf_beats_current_models` | `native/hayamimi-eval/tests/rp13.rs` | 原文を返した hyp |
| RP-14 | `rp14_chrf_matches_sacrebleu` | `native/hayamimi-eval/tests/rp14.rs` | n-gram の上限 5 |
| RP-15 | `rp15_der_not_worse_than_python` | `native/hayamimi-eval/tests/rp15.rs` | 再分離を切った run |
| RP-16 | `rp16_no_gpu_falls_back_to_cpu_and_says_so` | `native/hayamimi/tests/rp16.rs` | GPU が在るのに CPU へ落ちる build、落ちたのに event を出さない build |
| RP-17 | `rp17_self_built_cuda_library_opens_one_onnxruntime` | `native/hayamimi-gpu/tests/rp17.rs` | 配布の CPU 版に繋いだ build |
| RP-18 | `rp18_punctuation_matches_parity_fixture` | `native/hayamimi-asr/tests/rp18.rs` | NFKC を外した token 化 |
| RP-19 | `rp19_python_cases_are_ported` | `native/hayamimi-eval/tests/rp19.rs` | 名前を変えた list |
| RP-20 | `rp20_every_stage_placement_is_measured` | `native/hayamimi-eval/tests/rp20.rs` | 結果の行を消した file |
| RP-21 | `rp21_models_fetch_without_python` | `native/hayamimi/tests/rp21.rs` | 壊した取得先（もう 1 つの取得先か前の file へ落ちる） |
| RP-22 | `rp22_vram_stays_under_budget` | `native/hayamimi-eval/tests/rp22.rs` | 上限を run の起動前の読みの最大より 1 MiB 高く置いた設定 |
| RP-23 | `rp23_every_stage_is_timed_on_gpu_and_cpu` | `native/hayamimi-eval/tests/rp23.rs` | provider を指定しない run |
| RP-24 | `rp24_windows_and_wsl_builds_match` | `native/xtask/tests/rp24.rs` | ORT の版を変えた build |
| RP-25 | `rp25_translation_flicker_is_bounded` | `native/hayamimi-eval/tests/rp25.rs` | 先の訳を外へ送る偽の中継 |
| RP-26 | `rp26_messages_follow_the_locale` | `native/hayamimi/tests/rp26.rs` | 文言を直書きした build |
| RP-27 | `rp27_string_stages_do_not_import_ort_or_cuda` | `native/hayamimi-asr/tests/rp27.rs` | `use ort` を足した `itn.rs` |
| RP-28 | `rp28_cpu_fallback_latency_is_recorded_apart` | `native/hayamimi-eval/tests/rp28.rs` | CPU の run として渡した GPU の run |
| RP-29 | `rp29_missing_resources_fall_back_and_say_so` | `native/hayamimi/tests/rp29.rs` | 外したのに event を出さない build、在るのに落ちる build |
| RP-30 | `rp30_fallback_stage_and_reason_lists_match_the_code` | `native/hayamimi/tests/rp30.rs` | 1 つ消した一覧、code に無い値を足した一覧 |
| RP-31 | `rp31_latency_counts_only_shown_translations_of_their_own_utterance` | `native/hayamimi-eval/tests/rp31.rs` | `held` の空の訳を先に置いた列、遅れて着いた `final` |
| RP-32 | `rp32_alternated_runs_under_the_same_load_beat_python` | `native/hayamimi-eval/tests/rp32.rs` | load の記録が無い run、load 2 以上の run、交互でない順 |
| RP-33 | `rp33_first_translation_after_a_failed_draft_beats_python` | `native/hayamimi-eval/tests/rp33.rs` | 落ちた行を 600 ms 遅らせる偽の中継 |
| RP-34 | `rp34_wait_before_final_decode_not_longer_than_python` | `native/hayamimi-eval/tests/rp34.rs` | 確定の前に 300 ms 待つ偽の engine、GPU の記録が無い run |
| RP-35 | `rp35_latency_work_keeps_chrf_and_verify` | `native/hayamimi-eval/tests/rp35.rs` | V を掛けずに出す偽の中継、T の訳をそのまま確定にする偽の中継 |

test は会話字幕の設計 7.1 と同じ規則で書く（`Result` を返して `?`、`unwrap` と `expect` と `unwrap_or*` を書かない、実行時に環境変数を読まない）。
`CUDA_VISIBLE_DEVICES`、`CUDA_CACHE_PATH`、`LANG` は test の code が読む値ではなく、test が子の process を起こすときに渡す値である。

## 8. slice の順

各 slice の状態（済み、作業中、未着手）と merge の commit は `status.md`（日本語版は `status.ja.md`）に在る。
会話字幕の slice 1（`native/hayamimi-relay`、順 1〜13）は済んでいる。
R10 までの各 slice は Python の file を消さないので、slice の commit を revert すれば前の状態に戻る。
Python を消すのは R11 だけで、R11 の commit を revert すれば 78 本が戻る。

| slice | 中身 | 通す受入 | 戻し方 |
|---|---|---|---|
| R0 | 基準を固める。`hayamimi-eval` の `stream`、`record`、`fetch-set`、`fleurs-pairs` を作る。Windows の `.venv` に `pip install sacrebleu==2.6.0 jiwer==4.0.0 simpleder==0.0.5 OpenCC==1.4.2 pyarrow==25.0.1` を打つ（OpenCC が無いと yue の CER が t2s の正規化なしで黙って変わり、pyarrow が無いと FLEURS の対と評価の組が取れない）。今の Python を 2 回以上（DER は 3 回以上）動かし、遅れ、scorecard、golden、chrF の fixture、DER、wire の録りを `testdata/baseline/` に書く。`--min-silence` を 0.20、0.25、0.30、0.35 の 4 つで 2 回ずつ流して `L_asr` と ja の CER を測り、2 回とも CER が 0.35 の時を超えない最小の値を、R1 以降の Python と Rust の比較に使う値と Rust の既定に決める（0.30。`baseline_python.md:205-212`）。2 回で判定が割れた値（0.25）は採らず、R7 が Rust で 3 回以上流して決める。要約を `docs/results/baseline_python.md` に書く | RB-1〜RB-8（`rust_port_tests.ja.md`） | `pip uninstall` で 5 つを外す。crate を消す。Python の file は触らない。`docs/results/scorecard.md` は各回の前に `<base>` へ cp で退避し、後に cp で書き戻して `git diff --ignore-cr-at-eol --quiet` で確かめる（RB-3） |
| R1 | 計測の slice。最初の手番は 3.12 の「CUDA 13 版の ORT で sherpa-onnx が build でき RP-17 が通るか」で、通らなければ 3.12 の戻し方に従う。3.12 の環境を確かめ（WSL に cuDNN が無ければ入れる）、xtask で CUDA 版の sherpa-onnx を Windows（msvc）と WSL で build し、`hayamimi-gpu` で開き、`hayamimi-eval stages` で 3.5 の GPU の経路が在る段を GPU と CPU の 1 stream ずつ、無負荷と llama-server の負荷の下で測る。結果で 3.5 の表の置き場を決める。句読点の BERT は、`ort` と `punct_ja_parity.json` の `input_ids` で forward だけを CUDA と CPU に掛ける。認識は、encoder の model だけを `ort` で同じ fp32 の同じ入力に CUDA と CPU で掛ける 1 回と、fp16 の `gpu.session.placed` の CPU の node の数の 1 回を取る。3.5 の表で GPU とした model（言語判定の whisper-tiny の encoder と decoder を含む）の CUDA と CPU の node の数を、`gpu_stages.md` の node の表に全部載せる（RP-6 が読む） | RP-24、RP-17、RP-23、RP-7、RP-8、RP-20 | crate と結果の file を消す。環境は 3.12 の表の戻し方 |
| R1b | 評議。R0 と R1 の実測で、目標の数（RP-3 の 2000 ms、RP-7 の 20%、RP-22 の 14,000 MiB、RP-3 と RP-4 の対の比較の下限、根拠 3 の 1%）と、`--min-silence` の値を置き直す（6 の前置き、10 の H8 と H9）。WSL の build の置き場は 3.12 の環境の表のとおり | 評議の判定を この文書の 6 に写す | 文書の commit を revert |
| R2 | ja の経路（VAD、preroll、ReazonSpeech、分割再試行、ITN、句読点、置換辞書）、`--wav`、最小の server（`/events`）。3.11 と 3.15 の fallback の形（起動時の 2 段の確かめ、段ごとの CPU への落とし、`<domain>.fallback.<落ち先>` の event、`/events` の `fallback`、stderr の文）を作り、R2 の段に当てる（句読点の BERT の CPU への落とし、ja の経路の model の file が無いとき、推論の library が開けないとき、`/events` の port が使えないとき、設定の file が壊れたとき）。ITN と句読点の後処理の 1 行の CPU の p50 を `hayamimi-eval stages` で測り、`gpu_stages.md` の 2 段の CPU の行に置く。句読点の後処理は BERT を呼ぶ module と分け、`hayamimi-asr::punct::rules` に置く。作る crate は `native/hayamimi`（bin）と `native/hayamimi-asr`（lib）で、`native/Cargo.toml` の `members` に足す。どちらも `hayamimi-gpu` に依存するので、3.4 の link 引数 2 本を bins と tests に渡す build script を両方に置く。Windows の build は `SHERPA_ONNX_LIB_DIR` を `native/target/sherpa-gpu/windows/install/lib` に向ける。T-RP-6-012 と 013 は `native/hayamimi/tests/rp06.rs` に置く。R2 では `/ingest` が無いので `L` と `L_asr` は測れない。`asr.utterance.timed` の `l_dec_ms` を `--wav` の lat-ja で記録し、Python の `L_dec` の p50 311 ms と p95 446 ms（`gpu_stages.md` の「Python（記録）」の `L_dec` の行、:297）と並べる。合否には使わない。`L_asr` は R5 以降、`L` は R8e で測る | RP-2、RP-18、RP-6、RP-16、RP-26、RP-29 の R2 の 4 行（model の file、推論の library、port、設定の file）。RP-8 の根拠 4 の式（ITN と句読点の後処理の CPU の p50 が GPU の往復の下限より小さい）、RP-27（`itn.rs` と `punct/rules.rs` が在り、`rg` の rc が 1） | crate を消す |
| R3 | 言語の振り分け（whisper-tiny と SenseVoice の二重確認、粘り、5 つの tier、Omnilingual、LRU、清書、second opinion、ko の分かち書き）。言語判定の CPU への落とし、GPU の memory の上限での LRU と CPU への落とし、振り分けの認識器と言語判定の model が無いときの落とし（3.15）。R3a（振り分け）と R3b（認識器、常駐、`hayamimi-eval scorecard`、`lat-multi`）の 2 席に分ける（「07:00 の計画」）。言語判定を `hayamimi` の実行 file に繋ぐ（`Engine::open` で `lid::open` を呼び、本物の run で言語判定の event を出す）のは R3b である。session を作る所の process 全体の lock（3.4 の「session は 1 本ずつ作る」）も R3b が `hayamimi-gpu` に置き、R2 の句読点と R4 の話者の session もその lock を通す。話者の session を lock に通すのは、R3b と R4 のうち後に main へ merge する側である。lock の受入は T-RP-16-036 で、lock の無い形の abort（9 回で 2 回）は再現の稀な物なので T-RP-16-037 は記録だけにする（lock を外した 3 つの構成の 20 回ずつで 0 回だったことも記録に書き、合否に使わない）。RP-1 の合否を決める run は、Windows の配る形の build（6 の RP-1）で打つ。その build で振り分けを通すと、zh と yue は Python と同じ値になる（10 の H35。NOT VERIFIED で、検証の席が可を返したら外す）。R3b の merge の後に、書いていない検証の席が RP-10 を `lang` の条件つきで Windows で打ち直す（8 の R5 の行）。R3b の branch が 3.13 に足した 6 つの event（`asr.model.loaded`、`asr.model.evicted`、`asr.lang.unknown`、`gpu.memory.unreadable`、`model.fallback.substitute`、`model.fallback.chunks`）は、名前の形（`<domain>.<action>.<outcome>`）と 3.15 の落とし方に合うので採る | RP-1、RP-22、RP-16 の言語判定の行、RP-29 の R3 の行 | 同上 |
| R4 | 話者（CAM++、pyannote、FastClustering、全体の対応づけ、仮の印）。FastClustering と全体の対応づけの CPU の p50 を `hayamimi-eval stages` で測り、`gpu_stages.md` の「清書の clustering」の CPU の行に置く。話者の model が無いときは印なしで動かす（3.15）。確定文は話者の埋め込みを待たずに `speaker` を空で出し、埋め込みの後に `{"type": "speaker", "id": <確定文の id>, "speaker": <印>}` を同じ `id` へ 1 回出す（字幕を出す thread に埋め込みの約 70 ms を載せないため）。印の決まらない発話には出さない。確定文が話者を待たないことは、遅れの差ではなく、同じ `id` の `asr.final.published` が `speaker.embedding.started` より前か同じ時刻であることで確かめる（3.13。遅れの差は run ごとに約 40 ms 揺れ、待つ形と待たない形を見分けられない） | RP-15、RP-29 の R4 の行（T-RP-29-027、028）、T-RP-15-012 と 013。RP-8 の根拠 4 の式（清書の clustering の CPU の p50 が GPU の往復の下限より小さい） | 同上 |
| R5 | 取り込み（wasapi、cpal、mix、`/ingest`）と server の全 route。速報（伸びていく音声を約 0.5 秒ごとに解き直し、`partial` の event を出す。2.2）。`<ev> stream --sink loopback`（wav を既定の出力の装置へ鳴らし、送った sample の時刻を記録する）。取り込みから認識器へ渡すまでと、確定文の event を `/events` へ書き出すまでの CPU の p50 を `hayamimi-eval stages` で測り、`gpu_stages.md` の 2 段の CPU の行に置く。取り込みの装置が無いときと、全 route の port が使えないときの落とし（3.15）。確定文の `source`（会話字幕の設計 3.3）は `--input` から決め、`wav` は `file`、`mic` と `mix` と `ws` は `mic`、`loopback` は `loopback` とする。`--source <mic|loopback|file>` で上書きでき、`/ingest` で loopback の音を受ける process（E2E-2 の陽性対照の process B）は `--source loopback` を付ける | RP-10（`lang` が `en` の条件を除く。英語の経路は R3b が `--lang en` と振り分けを実行 file に繋ぐ時に入り、R3b の merge の時に RP-10 を `lang` の条件つきで打ち直す。R3b は W3 の終わりまでに merge する。E2E-2 が英語の経路を要るため）、RP-11、RP-12、RP-29 の R5 の行。RP-8 の根拠 4 の式（取り込みと配信の CPU の p50 が GPU の往復の下限より小さい） | 同上 |
| R6 | Dioxus の 3 頁（designer の板が先）。`hayamimi` の server が `/`、`/dashboard`、`/transcript` で `hayamimi-ui` の `web/index.html` を返し、`/ui/*` で wasm の出力を配る結線は、R5 と R6 のうち後に main へ merge する側が R5 の route に足す | RP-12 の頁の行と、overlay の同一性の test。結線の受入は、本物の `<hy> --wav tests/golden/ja/ja_014.wav --serve 0` を起こし、`/`、`/dashboard`、`/transcript` の GET が `web/index.html` と同じ bytes を返し、その index.html が読む `/ui/` の下の wasm と js の GET が全部 200 であること（陰性対照は、`/ui/` の下に無い名前の GET が 404） | crate を消す |
| R7 | 認識の遅れの詰め。R1 で遅いと分かった段を直す。3.5 の CPU の時間は 1 stream で測った値で、mic と loopback の 2 音源同時の CPU は未測定なので、2 音源を同時に流した CPU の段の p50 と p95 を測る。認識は 2 音源同時で GPU（fp32）と CPU（同じ fp32 と int8）を測り直し、CPU の側が GPU より遅くなれば 3.5 の置き場を評議へ戻す。認識全体の cuda の時間のうち encoder 以外の約 99 ms の内訳を調べる（10 の H31）。R7 の 1 回目の値（peers 1 の認識全体の p50 が GPU fp32 360316 µs、CPU fp32 487816 µs、CPU int8 396586 µs）は load 10〜32 の下で、CPU と CUDA を続けて取っていないので、置き場の判断に使わない（NOT VERIFIED）。測り直しは R7 の席が W4 の初め（04:45〜05:05）に打つ。その 20 分は他の席が cargo の build を打たず、打つ前に `/proc/loadavg` の 1 分の値が 2 未満であることを記録し、GPU fp32 と CPU int8 を交互に 3 回ずつ（GPU、CPU、GPU、CPU …）流す。CPU int8 の p50 が GPU fp32 の p50 より大きく出た時だけ 3.5 の置き場を評議へ戻し、その時に H31 の内訳を調べる（CPU が速ければ認識は GPU に載らず、H31 は置き場に効かない）。`compare.rs` のうち、Python と Rust の run の `L` と `L_asr` を比べる部分は R8e の RP-3 の道具を正とし、R7 は段の時間の比べだけを持つ。重なった関数は、後に main へ merge する側が片方に寄せる。H9（`--min-silence` 0.25）と H10（確定の解き直しを省く）は R7 が持ち、W4 の測り直しの後に同じ低い負荷の窓の外で打つ。W4 の中で終わらなければ、E2E の後に R7 の席が打つ | RP-4、RP-7 | 設定と commit を revert |
| R8a | 中継が Rust の `hayamimi` の `/events` を購読する（3.3 の 5 項目と `src_sha256`）。5 項目を `hayamimi` の確定文の event に足す。`<ev> stream` が中継の SSE の訳も読む | 会話字幕の CS-21、CS-22 | 中継の購読先を戻す |
| R8b | llama-server を Windows の native に置く（`%LOCALAPPDATA%\hayamimi\models\llm\`、slot 4）。起こす側が 3.11 の確かめを打ち、GPU が無ければ `-ngl 0` か CPU の build で起こし、`gpu.fallback.cpu`（`stage` が `translate`）を出す。llama-server に届かないときは訳を保留して原文を出し続ける（3.15） | 会話字幕の CS-20、RP-16 の訳の行、RP-29 の R8b の行。Windows の CUDA 13.4 の llama-server で T を測り直し、`gpu_stages.md` の T の行を置き換える（RP-23 の T の行の形） | WSL の llama-server に戻す |
| R8c | T の訳を先に出し、E の訳で置き換え、確定の訳だけを外へ出す。外へ出す口は、確定の訳（`settled: true`）だけを受ける送り先の trait 1 つにし、Zoom、Teams、SRT の中身は R8g が作る。外の送り先に届かないときは SRT と重ね窓に残す（3.15）。会話字幕の T-CX-1-002 は test-designer（sonnet）の席を R8c と同じ時に出して書き替え、R8c の検証の前に入れる（V に渡る文字列は E の応答で、V は T の応答と E の応答に 1 回ずつ、多くて 2 回）。wire の文書（会話字幕の設計 3.3）と実装の差は、R8c が差の一覧を返し、設計の席が同じ波の検証の前に 3.3 を直す | RP-25、会話字幕の CS-5、CS-25、RP-29 の R8c の行 | 中継の commit を revert |
| R8d | 速報の訳（re-translation、最長共通接頭辞、打ち切り、`cache_prompt`）。確定した字形で描いた接頭辞を後で消さないこと（RP-25 の 3 つ目の条件）を中継で守るのも R8d で、R8d はそれが未だでも merge してよく、R8d の続きの席が W3 の R8e の計測より前に入れる。R8e は RP-3 の run でその回数を測る（T-RP-25-010）。RP-5 の対象の発話（6 の RP-5 の陽性対照）に合わせ、test-designer の席が T を直す（セラが運ぶ）。T-RP-5-007 の対象外を「`final` の無い発話」に替え、`partial` だけが在って `final` の無い発話を 1 つ含む fixture で、その発話が p95 に入らず、別の数として 1 と数えられる行を足す | RP-5、RP-25 | 設定で速報の訳を切る |
| R8e | 4 つの slot と負荷の下の `L`。CPU に落ちた run の `L` を測り、RP-28 の仮の上限を置き直す | RP-3、RP-28 | slot の数を戻す |
| R8f | `--translate` の言語を llama-server の T で訳す。中継の `eval`、chrF の Rust の実装と `hayamimi-eval chrf`（sacrebleu 2.6.0 と同じ値）も R8f が持つ。`hayamimi-relay live --translate` に ja と en の外の言語（zh、ko、es ほか）を出す経路（`Translation.lang` を ja と en の 2 つに限らない形）も R8f の受入に入れ、W3 の終わりまでに入れる（E2E は ja と en だけなので止めない）。表に無い言語は 3.15 の `translate.fallback.skipped` で落とす。zh、ko、es の V は形、数、ラテン文字の名前だけを見る（10 の H36） | RP-13、RP-14。T の case 行は T-RP-13-010〜015 で足りる。加えて、R8f の検証の前に test-designer が 2 行を足す。1 行目は 014 の対で、表に在る言語だけの pairs では `translate.fallback.skipped` が 0 件。2 行目は live の経路で、偽の llama-server に `--translate zh` で 1 行流し、`lang` が `zh` の `translation` が 1 件。live の test は `parse_args` の返す設定を、`cli::run` が使うのと同じ「中継を起こして handle を返す関数」に渡して起こし、bind した port の `/events` を読み、handle で止める。`cli::run` はその関数を呼んで待つだけの薄い関数にし、走り続ける `cli::run` を test から直に起こさない。この形に合わせて、test-designer の席が T に 2 行を足す（セラが運ぶ）。017 に「`lang` が `en` の `translation` が 0 件」を足すことと、live で表に無い言語を渡す行（`--translate xx,zh` で `translate.fallback.skipped` が `xx` に 1 件、`/events` の `fallback`（`to` が `skipped`）が 1 件、zh の訳は出続ける）である | commit を revert |
| R8g | 外の送り先の中身。R8c の送り先の trait に、Zoom の字幕 API、Teams の CART、SRT の書き出し（会話字幕の設計 3.10 と 3.13）を足す。指定は中継の `hayamimi-relay live --send <send.toml>`（Zoom の token と Teams の link）と `--srt <path>` で、どちらも無ければ送らずに動く。token か link が手元に無い機械では CS-17 と CS-18 を打たずに残る穴にする（state.json の `blocked_on` と同じ）。動画から SRT を作る `<relay> srt <mp4>`（会話字幕の設計 3.13）と、`send.toml` の権限の検査と token を log に出さないこと（CS-28）も R8g が作る。どれも E2E には要らない（E2E は mic と loopback の 2 本で、送り先と動画を使わない）。YouTube Live へは送らない（会話字幕の設計 1 の「作らない物」と 4。配信は OBS の字幕頁の焼き込みで足りる）。`send.toml` の形と送り方は会話字幕の設計 3.10、`srt` の手順は 3.13 に従う。`srt` は `hayamimi --input ws --source file --ws-port <空き port>` を起こし（`/ingest` の port は `--ws-port`。`--serve` は `/events` の HTTP の port で、`srt` は使わない）、`/ingest` に繋いでから作業 file の音声を送り、同じ接続で返る event から `final` を集める（3.1 の測りと同じ口）。`hayamimi --wav` を起こしてから `/events` に繋ぐ形を採らないのは、`/events` が過去の `final` を遅れた client に送り直さず、繋ぐ前の `final` が落ちるからである | 会話字幕の CS-14、CS-15、CS-17、CS-18（token か link が在るときだけ）、CS-28、RP-29 の R8c の行（送り先に届かないとき） | commit を revert |
| R8h | 主指標 `L` の差の詰め（3.16）。1：`stream` の集計の数え方（`held` と空の訳を数えない、`id` と `t1_s` で結ぶ）。2：T が V に落ちた行の最初の訳。B（R8f の V の直し）だけを進め、A は 4 の run の後に 3.16 の 4 つの条件を満たした時にだけ、E2E の後に作る。3：復号の順は変えず、確定の復号の前の待ちを `L_asr − decode_ms` で測る。待ちの log と、run の前後の `nvidia-smi` の記録を 4 の前に入れる。4：同じ負荷で Python と Rust を交互に測る | RP-31、RP-32、RP-33、RP-34、RP-35 | commit を revert。1 を revert すると `L` の数え方が前の形に戻る |
| R0b | worktree に `testdata/` が無いとき、test の support が本体の `testdata/` を引く。`hayamimi-eval` の `tests/support/mod.rs` の `BASE` と `TESTDATA` を、まず `CARGO_MANIFEST_DIR` の 2 つ上の `testdata`、無ければ `git rev-parse --path-format=absolute --git-common-dir` の親の `testdata` の順で探す関数にする。両方に無ければ、探した 2 つの path を名指しして `Err` を返す（黙って通さない）。symlink は使わない（Windows から WSL の symlink を辿れない。RP-24 の os error 1920） | worktree の中で `cargo test -p hayamimi-eval --test rb01 … --test rb08` の FAILED が 0。陰性対照は、本体にも worktree にも `testdata/` が無い clone で、2 つの path を名指しした `Err` で落ちる | commit を revert |
| R10 | `models fetch`、`config dump`、`doc-links`、README（`README.md` と `README.ja.md` の CPU だけという売りの行を含む）と文書の書き替え、英語版。`docs/guide/embedding.md` と `embedding.ja.md` の wire の記述を、その時の実装（3.3 の項目、`settled`、`of: "partial"`、`speaker` の event）に合わせる。T-RP-21-001、005、006、007 は `native/hayamimi/tests/rp21.rs` に置き、007 が使う RP-9 の `check_descendants` を R10 が作る | RP-21、RP-19 の spec の行、RP-30 | 文書の commit を revert |
| R11 | 3.2 の 3 つの条件を確かめてから、Python の 78 本と `requirements*.txt` を消し、CI を cargo に替え、Dart の comment を書き替える | RP-9、RP-19 | R11 の commit を revert |

R0 で Python を動かすのは、置き換える前の物の速さと誤り率を同じ機械で測るためで、Python の file は 1 行も書かない。
2 回以上動かすのは、回の間の揺れを数えて、Rust との差が揺れの中に入っていないかを読むためである。
DER は 3 回以上流す。2 回では回の間の幅を決められないからである。

次の 3 つの測定は、原因を確かめる物で、基準の値も受入も動かさないので gate にしない。
どれも R1 を止めずに、他の run と並べずに 1 本ずつ単独で流す。

- 発話 10 の区間に Silero VAD（threshold 0.5）だけを掛け、区切られた区間の数を数える。
- `e2f30ac` の版で IS1009a の DER を流す。worktree は repo の中の `.claude/worktrees/` に作る。CPU を使うので、他の run と同時に流さない。
- `--translate` を付けない Python の run で、言語判定をやり直さなかった `final` の `latency_ms` の平均を取る。

R1 を認識の実装より先に置くのは、速度が第一で、どの段を GPU に置くかを実測で決めてから作るためである。
R1 の `stages` は、sherpa-onnx の crate の型を直に呼ぶ小さな計測の道具で、R2 以降の経路の code に依らない。

### 07:00 の計画

R3〜R11 の全 slice を、2026-10-15 07:00 JST までに main へ入れる。
範囲は削らない。
実装の席は波ごとに最大 5 席で、席ごとに repo の中の `.claude/worktrees/` に worktree を分ける。
波の後に、書いていない席が受入を打ち直してから merge する。

**依存**（どの slice がどの成果物を要るか）：

| slice | 要る物 | 根拠（今の木） |
|---|---|---|
| R3a、R3b | R2 の ja の経路と確定文の event | `native/hayamimi/src/run.rs:64` の `final_event` は `lang` が `"ja"`、`tier` が `"rz"` の固定 |
| R4 | R2 の確定文の event（R3 に依らない。話者は発話ごとに印を足す） | 同じ `final_event` の `speaker` が空 |
| R5 | R2 の server と CLI | `native/hayamimi/src/server.rs:105` の route は `/events` だけ。`native/hayamimi/src/args.rs:16` の入力は `--wav` だけ |
| R5（速報） | 速報の event を出す者が今は居ない | `run.rs:64` は `final` だけを出す。Python は `scripts/realtime_transcribe.py:895` の `hub.partial` |
| R6 | designer の板と、R0 の wire の録り（3.8）。頁の route は R5 | 3.8 の「designer に渡す物」 |
| R7 | R5 の 2 音源の取り込みと、R3 の言語ごとの認識器 | 8 の R7 の行 |
| R8a | 中継に `/events` を購読する口が無い。確定文に 3.3 の 5 項目が無い | `native/hayamimi-relay/src/cli.rs:21` の `Command` は `FxFetch` と `FxSchedule` だけ。`run.rs:64` に `id`、`source`、`t0_s`、`t1_s`、`end_wall_ms` が無い |
| R8b | 無し（環境と起こす側） | 3.11 の訳の行 |
| R8c | R8a | 8 の R8c の行 |
| R8d | R8a と R5 の速報の event | 8 の R8d の行 |
| R8e | R5 の `/ingest`、R8a〜R8d、R8b | RP-3 の command（6）が `/ingest` と中継の訳を読む |
| R8f | R8b の llama-server。中継の `eval` は R8f が足す | RP-13 の command（6） |
| R8g | R8c の送り先の trait。E2E には要らない（E2E は中継の SSE と頁で字幕を読む） | 8 の R8c と R8g の行 |
| R8h | 1 は R8e の `stream` の集計。2 は R8c の先の訳と、R8f の V の直し（`feat/r8f-verify`）。3 は R5 の速報の loop。4 は R8e の run の command | `native/hayamimi-eval/src/latency.rs` の訳の結び方は着いた順（`positional_translations`）で `status` を読まない。`native/hayamimi-relay/src/pipeline.rs` の先の訳は `draft.passed` の時だけ出る。`native/hayamimi/src/session.rs` の `on_window` が速報と確定を同じ loop で回す |
| R0b | 無し。worktree の席の rb01〜rb08 を通すので、空いた席ですぐ打つ | `native/hayamimi-eval/tests/support/mod.rs:23` と `:25` が `CARGO_MANIFEST_DIR` の 2 つ上の `testdata` だけを見る |
| R10 | R3、R4、R5 の config（`config dump` が全項目を出す） | RP-19 の spec の行 |
| R11 | RP-1（R3）、RP-13（R8f）、RP-15（R4）、RP-19 の全行、R10 | 3.2 の R11 に入る 3 つの条件 |
| 端から端 | R3（`--lang`）、R5（取り込み）、R8a〜R8c、R6 の頁 | この小節の E2E-1 と E2E-2 |

R3 は 2 席に分ける。R3a は振り分け（`hayamimi-asr::route` と `::lid`、RP-19 の `route.rs` 70 行と `lid.rs` 9 行）、R3b は認識器と常駐（`::decode` と `::resident`、`decode.rs` 24 行、`hayamimi-eval scorecard`、RP-22）である。
R3a は R3b の `decode` を、言語の tier を受けて文字列を返す 1 つの関数として呼び、R3b が出来るまでは test の中の偽の関数で通す。

実装の席は `docs/design/status.md` と `status.ja.md` を書かない。slice を main へ merge した時に、設計の席がその slice の行の 4 列（状態、merge の commit、検証の出所、受入の通過）を英語版と日本語版の両方で書く（branch と main の間で status がぶつかるのを避けるため）。

**波**：

| 波 | 時間（JST） | slice | 見積り |
|---|---|---|---|
| W0 | 13:30〜14:00 | 設計の席：`gpu_stages.md` の引用の打ち直し（済み）、この計画、R3 の着手の 4 件。designer の席を出す | 0.5 時間 |
| W1 | 14:00〜20:00 | R3a、R3b、R4、R5、R8a。R3a の merge で空いた席に R0b（0.5 時間） | 各 4〜6 時間 |
| V1 | 20:00〜21:00 | 検証と merge（順は R8a、R4、R3b、R3a、R5。`args.rs` を足す席が後ろ） | 1 時間 |
| W2 | 21:00〜01:00 | R6、R7、R8b、R8c、R8d。R8c の merge で空いた席に R3c（RP-1 の zh と yue、10 の H35） | 各 3〜4 時間 |
| V2 | 01:00〜01:45 | 検証と merge | 0.75 時間 |
| W3 | 01:45〜04:15 | R8e、R8f、R8g、R10。空いた席に R7b（日本語の streaming の認識器を測るだけ。10 の H38）と R8h の 1 と 3（待ちの log、GPU の記録）。R8h の 2 の B は R8f の V の直しの席（3.16 の「順と置き場」） | 各 2.5 時間 |
| V3 | 04:15〜04:45 | 検証と merge | 0.5 時間 |
| W4 | 04:45〜05:45 | R11 | 1 時間 |
| E2E | 05:45〜06:45 | 端から端の 2 本と `L` の p50、p95。`L` は RP-32 の交互の run で取る | 1 時間 |

cargo の並列は 2 に固定されている。
5 席が同時に `cargo build --release` を打つと 10 job になり、この機械の 16 core に収まるが、1 席の build は単独の時より遅くなる。
見積りは各席の時間に 2 割を足してある。
Windows の実機の test（RP-10、RP-16 の訳の行、R8b、端から端）は Windows の cargo を使い、同じ時に 2 席までにする。

**R3 の着手の 4 件**（止めずに進めるために決める。仮の値は持ち主と置き直す条件を添える）：

1. **scorecard**：R3b が `hayamimi-eval scorecard --engine hayamimi` を作る。`hayamimi-eval` が `hayamimi-asr` の振り分けを process の中で起こし、`<hy>` の既定と同じ設定（二重確認あり、en は Parakeet v3、second opinion なし、`--max-resident` 3）で、manifest ごとに session の状態を初めに戻して clip を 1 本ずつ解く。Python の `scripts/eval_engine.py` が `RoutedASR(threads=6, preload=False)` を process の中で起こし、manifest ごとに `reset_session()` を打つのと同じ形である。clip ごとに `<hy>` を起こさないのは、model の読み込みが clip ごとに入り、言語の粘りの状態も切れて Python と同じ物を測らなくなるからである。
2. **RP-22 の実走**：R3b は RP-3 の run（`/ingest`、R5）を待たない。R3b の run は、`<ev> fetch-set --concat` で `testdata/manifests/eval_real.json` の ja と en、`eval_real_zhko.json` の zh と ko、`eval_real_yue.json` の yue をつないだ `testdata/lat-multi.wav` を、llama-server を起こした機械で `<hy> --wav testdata/lat-multi.wav`（実時間の速さ）に流し、その間の読みで T-RP-22-007〜009 を打つ。仮の形で、持ち主は R8e、置き直す条件は R8e が RP-3 の run の間に同じ読みを取り直すことである。
3. **LRU の対象**：LRU は CPU に常駐させる認識器の数を縛る（`--max-resident`、既定 3、ReazonSpeech は数に入れず常に残す。Python の `scripts/asr_engine.py:788` と `:1166`）。認識器は 3.5 で CPU に置くので、GPU の memory からは外す物が無い。GPU の memory の上限（14,000 MiB、仮の値、持ち主 R3b と R8e）は GPU 全体の `memory.used` の最大で、llama-server を含む。上限に届いたら、GPU に置いた段を `L` の損の小さい順に CPU へ落とす。順は句読点（`gpu_stages.md` の「置き場の提案」の句読点の行、:264 で GPU 8 ms 対 CPU 45 ms）、言語判定（同じ表の言語判定の行、:259 で 76 ms 対 124 ms）、訳（R8b の形）である。5 言語の振り分けで使う認識器は ReazonSpeech の外に 3 つ（Parakeet v3、Paraformer、SenseVoice）なので、既定の 3 では追い出しが起きない。追い出しの確かめは `--max-resident 2` の run で行う。
4. **振り分けの仕様**：振り分けは Python の意図を移し、code の形は写さない。合否を決めるのは、RP-19 の対応表の `route.rs`、`lid.rs`、`decode.rs` の行の case と、RP-1 と、`config dump` の値（RP-19 の spec の行）である。この 3 つが決めない振る舞いは `scripts/asr_engine.py` の挙動に合わせ、合わせられないときは評議へ出す。既定で読まない model が無いときは、3.15 の model の行の順で落とす。Parakeet v2（`--en-tier v2`）は Parakeet v3 へ、parakeet-ja（`--refine-ja-second-opinion`）は second opinion を切る（`model.fallback.disabled`）。RP-1 は既定の設定（en は v3、second opinion なし）で測るので、2 つの model の有無は en と ko の閾値に効かない。RP-1 の run に `model.fallback.*` の event が 1 件でも在れば、既定の経路を測っていないので RP-1 は閾値を緩めずに落ちる。

**designer の席に頼む物**（W0 で出し、W1 の間に板を返させる。R6 は板が在ってから W2 で作る）：

- 3 頁（dashboard、OBS の overlay、transcript）の Figma の板。台帳の path と板の URL を返させる。
- 描く要素は 3.8 の「designer に渡す物」の全部（速報の帯、確定の行と言語の印と話者の印と行ごとの遅れ、訳、清書の列、`?show=final` と `?show=partial`、速報の訳の確定の部分と薄い部分、先の訳を確定の訳で置き換える形）と、3.15 の fallback の印。
- 今の頁は `scripts/subtitle_server.py` の `OVERLAY_HTML`（:14）、`TRANSCRIPT_HTML`（:57）、`DASHBOARD_HTML`（:83）と、mobile の `overlay_html.dart` を読ませる。overlay は `overlay_html.dart` と同じに描く条件（`tests/test_overlay_parity.py`）を外さない。
- 返す物は、板の URL、token（色、字、間隔）の表、頁ごとの要素と event の型の対応表。

**端から端の受入**（E2E の波で、書いていない席が Windows の実機で打つ）：

| process | 音源 | 認識 | 訳 | 字幕の port | `/ingest` の port |
|---|---|---|---|---|---|
| A | マイク | ja（`--lang ja`） | en | 8833 | 8766 |
| B | スピーカーの loopback | en（`--lang en`） | ja | 8834 | 8767 |

中継（`hayamimi-relay live`）の既定は次の 3 つで、E2E では引数なしの `hayamimi-relay live` で起こす。

| option | 既定 | 持ち主 | 置き直す条件 |
|---|---|---|---|
| `--llm` | `http://127.0.0.1:8080`（llama-server の `--port` の既定） | R8b | R8b が llama-server を別の port で起こす形にしたとき、同じ commit でこの既定を替える |
| `--serve` | 8835（process A と B の字幕の port の次） | R8a | 使えなければ 3.15 の port の行のとおり次の 10 個から空いた port を使い、実際の URL を出す。8835 を他の常駐物が使うと分かったら R8a が替える |
| `--events` | `http://127.0.0.1:8833/events` と `http://127.0.0.1:8834/events` の 2 本 | R8a | 字幕の port の表が変わったとき。片方に届かなくても止めず、届く方だけで訳し、届かない方を、待ちを 250 ms から倍に伸ばし、上限 5 秒で繋ぎ直す |

| ID | command | 期待値 | 陽性対照 | 陰性対照 |
|---|---|---|---|---|
| E2E-1 | llama-server、中継、process A を起こし、`<ev> stream --wav testdata/lat-ja.wav --repeat 5 --port 8766 --out e2e1.jsonl`、`cargo test --release --test e2e1 -- --ignored` | 有効な標本（3.1）のうち訳の取れた割合が 20 / 23 以上。`L`、`L_asr`、`L_tr` の p50 と p95 を出す。`L` は RP-3 の期待値（Python の p50 と p95 以下）で、RP-32 の同じ負荷で交互に測った Python の run と比べて判定する | 同じ test が `<base>` の Python の run から同じ列を出せる | llama-server を止めた run では訳が 0 件で test が落ち、`hayamimi` と中継は止まらず `translate.fallback.held` を出す |
| E2E-2 | llama-server、中継、process B を起こし、`<ev> stream --wav testdata/lat-en.wav --repeat 5 --sink loopback --out e2e2.jsonl`（既定の出力の装置へ鳴らし、送った sample の時刻を記録する口は R5 が足す）、`cargo test --release --test e2e2 -- --ignored` | `lang` が `en` の `final` と `lang` が `ja` の訳が出て、有効な標本のうち訳の取れた割合が 20 / 23 以上。`L` の p50 と p95 を出し、p95 が 2000 ms 以下（RP-3 の en→ja の値） | 同じ音声を `/ingest`（8767）へ送った run でも同じ列が出る | 出力の装置を止めた run では止まらず `capture.fallback.none` が 1 件 |

マイクの実機（RDP の録音の転送を通る音声）は、process A を起こして `capture.fallback.none` が 0 件で、マイクの装置の名前が log に出ることまでを機械で確かめる。
人が話した ja で en の字幕が出ることは、話す者が居る時にしか打てない（NOT VERIFIED）。
E2E の画面の確かめでは、R6 の頁に fallback の帯が出た状態（llama-server を止めた陰性対照の run）の DOM で、帯の高さと `role="status"` を R6 の板（`r6_board.ja.md` の 4.3）と照らす。R6 の画面テストはこの状態を測っていない。

| ID | 受入 test の名前 | 置き場 |
|---|---|---|
| E2E-1 | `e2e1_mic_process_ja_to_en_beats_python` | `native/hayamimi-eval/tests/e2e1.rs` |
| E2E-2 | `e2e2_loopback_process_en_to_ja_within_budget` | `native/hayamimi-eval/tests/e2e2.rs` |

## 9. 決めたこと

評議の判定（2026-10-13）で決めた物は、この文書の本文に写した。

| 項目 | 決め | 本文 |
|---|---|---|
| T の訳を先に出す | 採る。V は多くて 2 回、確定の訳だけを外へ出し、揺れを RP-25 で数える | 3.6、会話字幕の 3.4、3.10、3.13 |
| 会話字幕の設計と goals.md の書き替え | この文書と同じ手番で書き替えた。README の 3 行は R10 | 2.4 |
| R0 で Python を動かす | 2 回以上（DER は 3 回以上）。採点と評価の 5 つを `pip install` する | 8 の R0 |
| 要る物が無い機械と場面 | 止めずに段ごとに CPU、小さい model、縮めた形へ落とし、event と画面に出す | 3.11、3.15、RP-16、RP-28、RP-29 |
| llama-server の置き場 | Windows の native | 3.6、会話字幕の 3.7 |
| Windows の target | `x86_64-pc-windows-msvc` にそろえる | 3.12 |
| Python を消す時 | R11 だけ。3 つの条件の後 | 3.2 |

## 10. 残る穴

- **H1** Parakeet v3（en と欧州 24 言語）と parakeet-ja（second opinion）の fp32 か fp16 の ONNX は、sherpa-onnx の release に見つからない（NOT VERIFIED）。R1 の席が Hugging Face を探し、無ければ int8 を CUDA で動かして CPU に落ちる node の数を `gpu.session.placed` で数え、R1 の表に載せる。そのとき 1.4 の既定を満たせない段として評議へ戻す。
- **H3** `ort` 2.0.0-rc.14 の `load-dynamic` が、自分で build に入れた ORT 1.31.0 を開けるか（NOT VERIFIED。1.31 は `ort` の既定の C API の版で、退路の 1.28.3 では既定を外して古い API の feature を選ぶ）。別の席の調べでは `ort` の配布の binary は CUDA 13.2 以上と cuDNN 9.23 以上を対象にしている（S15）。`load-dynamic` は配布の binary を使わないが、C の API の版の下限が合うかは R1 の RP-17 で確かめる。合わなければ、句読点の model を sherpa-onnx の側で動かす手段を評議へ出す。
- **H5** CUDA の stream を ORT と llama.cpp で共有する手段は無く、同じ GPU を 2 つの process が時間で分け合う。負荷の下の時間は R1 の RP-23 と、RP-3 で測る。
- **H44** RP-5 の実走（T-RP-5-009）が落ちる。R8e の Rust の run の `t_partial` の p95 が 4398 ms で、同じ `--min-silence` 0.30 の Python の 3751 ms を超える。原因は特定していない（NOT VERIFIED）。持ち主は R8h で、3.16 の 3 の待ちの log と RP-32 の交互の run で速報の遅れを Python と並べ、差が残れば原因を評議へ出す。
- **H46** 3.15 の「llama-server に届かない」の行の、5 秒ごとの `/health` の確かめは作っていない。`live` は依頼の時にだけ届かないことを検出するので、確定文が来ない間は `translate.fallback.held` が出ず、画面の印も遅れる。E2E には要らない（E2E-1 の陰性対照は llama-server を止めて音声を流すので、依頼の時の検出で `translate.fallback.held` が出る）。持ち主は R8h の後に中継を触る席で、E2E の後に作る。受入は RP-29 の R8b の行に、確定文を送らずに llama-server を止め、6 秒以内に `translate.fallback.held` が 1 件出る case を足す（test-designer の席が書く）。
- **H7** 発話の終わり `t_end` は manifest に書いた sample の位置で決める。実放送の音声は参照の境界と本当の発話の終わりがずれることがあり、`L` に数十 ms の誤差が入る（ASSUMPTION）。Python と Rust に同じ誤差が入るので、比較には効かない。
- **H8** RP-3 の en→ja の 2000 ms、RP-7 の 20%、RP-22 の 14,000 MiB は、実測の裏づけが無いまま据え置いた値である（NOT VERIFIED）。lat-en の Python の基準が無く、R1 の `L` の p95 1543 ms と 1890 ms（`gpu_stages.md` の「遅れ L の見積り」の「Rust 認識 cpu / 訳 cuda 無負荷」と「負荷」の行、:307-308）は見積りで、RP-7 の R1 の値は認識を CUDA に置いた構成の値で、`gpu_stages.md` に memory の実測が無い。持ち主は 2000 ms が R8e、20% が R7、14,000 MiB が R3 と R8e で、測って超えたら評議へ戻す。RP-3 と RP-4 の対の比較の下限（比較集合の発話の数 × 20 / 23 の切り上げ）と根拠 3 の 1% は、R0 と R1 の実測で裏づけた（6 の RP-3、3.5）。
- **H8b** RP-28 の CPU に落ちた時の `L` の上限（p50 4500 ms、p95 7000 ms）は、R1 の段の和の見積りに約 1 割を足した仮の値で、実測ではない（NOT VERIFIED）。R8e が測って置き直す。
- **H8c** RP-6 の 256 MiB と 64 MiB は、Windows の 1 回の実測（基準 1198 MiB、最大 1957 MiB）から置いた仮の値である（NOT VERIFIED）。`memory.used` は GPU 全体の値なので、ほかの process が同じ時に memory を取ると誤って通る。RP-6 はほかの GPU の process を止めた機械で打ち、`gpu.session.created` の provider と合わせて読む。R2 が WSL でも同じ読みを取り、R1b と同じ評議で置き直す。
- **H9** `--min-silence` は `L_asr` の最大の部分である。比較の値と Rust の既定は 0.30 で（8 の R0、`baseline_python.md:205-212`）、2 回の run で判定が割れた 0.25 は R7 が Rust で 3 回以上流して決める。
- **H10** 確定のとき、最後の速報の解き直しと確定の音声が同じなら、確定の解き直しを省ける見込みがある（ASSUMPTION）。R7 で、同じ音声かを判定する条件と、省いた時の誤り率を測る。
- **H11** sherpa-onnx の中の fbank と探索を分けて測る口が無いときは、3.5 の差の式で出すので、値は推定になる。R1 の結果の file にそのことを書く。fbank か探索が遅い段だと分かっても、sherpa-onnx の中を替えるのは C++ を書く変更になるので、評議へ出す。
- **H12** Nsight Systems が Windows の Rust の実行 file を profile できるかは確かめていない（NOT VERIFIED）。できなければ WSL の同じ build で数える（3.5）。
- **H13** Kiwi（ko の分かち書き）の Rust の binding `kiwi-rs` は Kiwi の C の library を要し、LGPL である。今と同じく任意とし、無いときは素通しにする。Windows の msvc の build から読めるかは R3 で確かめる（NOT VERIFIED）。
- **H14** `docs/results/scorecard.md` の評価の音声は手元に無い（`testdata/eval_real` が無い）。R0 の `fetch-set` が取り直す。取り直した音声が scorecard.md の時と同じかは、R0 の Python の値が scorecard.md と 0.005 以内で一致すること（RB-3）で見る。
- **H16** 英語版 `rust_port.md` は R10 の成果物である。それまでこの文書の日本語だけが在る。
- **H17** CAT-Translate-3.3b の GGUF は配布が無く、会話字幕の試しでは自分で変換した（会話字幕の設計 2.3）。変換は Python の道具（llama.cpp の `convert_hf_to_gguf.py`）を使う。訳の model を決める会話字幕の設計 9 の Q1 が CAT を選んだときに、変換を Python 以外で行う手段が無ければ、その 1 回の変換を規約の例外とするかを評議へ出す。
- **H18** 別の席の調べの「GPU の方が遅い」「GPU の道具が無い」は推測を含む転記で、この文書は根拠に使っていない。3.5 の根拠は R1 の実測と一次資料である。
- **H19** 退路の ORT 1.28.3 は両 OS とも Microsoft の asset である。WSL の build は archive の sha256 が GitHub の digest と一致し、RP-17 を通した（実測、2026-10-14）。Windows の 1.28.3 の build は作っていない（NOT VERIFIED）。退路へ戻すときは Windows でも作って RP-17 を通し、RP-24 で 2 つの OS の版の名前をそろえて記録する。
- **H20** hooks の `guard_commands.rs` の llama.cpp の鮮度の検査は、`llama-` で始まる名前を含む command を、WSL の build の木の鮮度で判定する。Windows の llama-server を WSL の shell から起こすと、関係の無い木で refuse される。Windows の llama-server は PowerShell から起こすと決めた（3.6）が、検査の対象を名前でなく path で分ける直しが要るかは、R8b の手番で hook-engineer の席へ出す。
- **H24** `ferrous-opencc` の t2s が、Python の OpenCC 1.4.2 の t2s と同じ文字列を出すかは確かめていない（NOT VERIFIED）。R3 で、R0 の yue の参照文と認識文の全件を両方に通し、差の行の数を数える。差が在れば、yue の CER の差がその行で説明できるかを RP-1 の結果に書く。
- **H22** `state.json` の `constraints` に「Rust への移植は速度が足りないと測れたときだけ」が残っていて、この文書の目標と食い違う。state.json を書くセラが直す。
- **H23** 速報の訳が `final` と同じ原文になったときの `L_tr` の負の値は、速報を早く出す形の正しい効果だが、その発話の数が多いと `L` が小さく見える。RP-3 の出力でその数を見て、多ければ R1b で読み方を決める。
- **H25** Python の `asr_engine.py` の 1715 行目は、早めに判定した言語を 4 秒未満の発話にだけ使い、それより長い発話では確定のたびに言語判定をやり直す。やり直し 1 回あたり 114 ms で、`L_asr` に入る。Rust でこの規則を写すか変えるかは、R1 の言語判定の GPU と CPU の実測（3.5）の後に R3 が決める。
- **H26** `end_sample` の今の規則（20 ms の frame の音量）は、声の後ろに音楽が続く発話（lat-ja の 18 と 19）で声の終わりを決められず、その標本は終端不正に入る。全発話に当てる別の規則（Silero VAD の区切り、手で付けたラベル）で直すかは決めていない（NOT VERIFIED）。発話 18 と 19 は Python の訳が 1 回も取れないので、`L` の比較集合には入らない。直すなら 3.1 の規則のとおり全発話を置き直し、R0 の席が Python を測り直す。
- **H27** 機械語の当たらない kernel を driver が PTX から変換する初回の遅れは測っていない（NOT VERIFIED）。R1 の RP-23 で、JIT の cache が空の run（子の process に `CUDA_CACHE_PATH` で空の directory を渡す。S18）と、同じ cache で続けて流した温かい run の `decode_ms` を別の行に記録する。差の読み方は R1b で決める。
- **H28** Windows で `onnxruntime.dll` の import が System32 の 1.17.1 に結びつく条件は特定していない（NOT VERIFIED）。3.4 の遅延読込みと先読みは probe の 4 回で防げたことだけを確かめている。test の実行 file は cargo が `target/<triple>/<profile>/deps/` に置き、`sherpa-onnx-sys` が DLL を写す `target/<triple>/<profile>/` と directory が違うので、test の process で「実行 file の隣」を先読みしたときに `onnxruntime.dll` が在るかも確かめていない（NOT VERIFIED）。R1 の席が `rustc-link-arg-tests` を足す手番で、test の process の `modules` の origin を数える。
- **H29** 3.5 の「負荷の下」の値は、WSL の llama-server と Windows の ORT が同じ GPU を分けた値で、本番の分け方（Windows の llama-server と Windows の ORT）ではない（NOT VERIFIED）。訳 T の GPU の値は `sm_120` を持たない CUDA 12.0 の build で測ったので、悪い向きに偏りうる（3.12）。R8b が Windows の CUDA 13.4 で T を測り直し、R8e が本番の分け方の負荷の下で `L` を測る（RP-3）。GPU の往復の下限（219 µs）も R1 の計測の値で、測り直していない。
- **H30** 認識の fp16 の GPU が fp32 の 2.7 倍遅い理由は未特定である（NOT VERIFIED）。`ort` の session で CUDA の時に CPU に置かれる node は fp32 が 1599、fp16 が 1611 で（`gpu_stages.md` の node の表の `encoder-epoch-35-avg-1.onnx` と `encoder-epoch-35-avg-1.fp16.onnx` の行、:246 と :245）、差の 12 node では説明できない。認識は CPU に置くので置き場には効かないが、GPU で fp16 を選ぶ段が他に出たときに同じ形で遅くなりうる。
- **H31** 認識全体の cuda の時間（同じ fp32 の無負荷の p50 171414 µs）のうち、encoder だけの時間（66262 µs）と fbank（約 6 ms）を除いた約 99 ms が何に使われているか説明できない（NOT VERIFIED）。転送は 1 発話あたり host → device 1524 µs と device → host 935 µs で（`gpu_stages.md` の転送の表の fp32 の 2 行、:221-222）、この差を説明しない。node の置き場は `ort` の session の値で、sherpa-onnx の session の置き場は確かめていない。持ち主は R7 で、内訳を調べ、encoder と探索を別の provider に分けずに縮める手段が在るかを評議へ出す。
- **H32** llama.cpp の CUDA の build が CUDA の device 0 の機械で起動し CPU で動くか、`-ngl 0` で cuDNN と cuBLAS を読まずに済むかは確かめていない（NOT VERIFIED）。R8b が `CUDA_VISIBLE_DEVICES` を空にして起こして確かめ、起動できなければ CPU の build（`llama-<tag>-bin-win-cpu-x64.zip`）を同じ置き場に並べる。sherpa-onnx が CUDA の実行 provider を作れないときに自分で CPU へ落とすかも確かめていないので、3.11 の 2 段目は `gpu.session.created` の実際の provider で判定する。
- **H35** RP-1 の誤り率の差は、Rust の code ではなく ORT の build の違いから出ている（R3c の実測。Python の wheel の ORT 1.27.1 を Rust に読ませると 66 clip が Python の log と小数 3 桁で一致し、自分で build した ORT では 5 clip が違う）。Windows の値（ja、en、zh、ko、yue の百万分率）は、Python と同じ library で 37512、22606、65609、80572、61454、配る形の自分の build で 37512、22606、65609、86939、61454 である。RP-1 の小数 3 桁の比べ方（6 の RP-1）では、配る形の build の ko の 0.087 だけが Python の 0.081 を超える。ORT の版は古くしない（常に最新の版を使う）。RP-1 の閾値も緩めず、ko の行は赤のままこの穴に残す（評議の判定）。Rust の code は基準（`testdata/baseline/scorecard_run1.md` の ko の行）と同じ物を出す。Python の wheel の ORT を読ませた run（`win_python_libs.json`）で ko が 80572（0.081）、66 clip が一致したからである。R11 の入口では ko の上限を配る形の build の記録値 86939（百万分率、丸めずに整数のまま比べる）にする（3.2 の 2）。SenseVoice を CPU の fp32 で読む形は採らない。`L` の延びが 1% 以内という条件を満たさないからである。fp32 の ko の 0.0937 と yue の 0.0912 は、VAD が欠けて長い音声を一定の長さで切る `model.fallback.chunks`（`decode.rs` の 416〜430 行）が入った値で、精度の効果とは読めないので引かない。R3b と R3c は ko をこの穴として残したまま merge してよく、E2E は ja と en だけを使うので止まらない。振り分けを通した Windows の配る形の build の run（`lid: detected`、記録は R3c の `docs/results/r3c/win_routed.json` と `h35_clips.md`）でも、ja 37512（LID 15/15、ja_02 は誤り率 0）、en 22606、zh 65609、ko 86939、yue 61454 で、正解の言語を渡した run と同じ値になり、言語判定の外れは 66 clip 中 0 である。zh と yue は Python と同じ値で、RP-1 を通る（実装の席の申告で、NOT VERIFIED。検証の席が可を返したら外す）。R3b の run の zh 0.0699 と yue 0.0682 は、この build では出ない。
- **H38** 日本語の streaming の認識器が出ている（`docs/research/04-recheck-2026-10.ja.md` の「速度と品質に効きそうな変化」）。今の認識は非 streaming の認識器を VAD の区切りに掛け、`--min-silence` の無音を待ってから解くので、`L_asr` に無音の待ちと発話全体の解き直しが入る。streaming の認識器は、話している間に解き進めるので、`L` を縮められる見込みがある（NOT VERIFIED）。`goals.md` の 56 行目の「高精度な日本語の streaming model が出たら増分デコードを再評価する」の条件に当たる。候補は 3 つで、Rust の実行時の経路に載るかが違う。Nemotron 3.5 ASR Streaming 0.6B（ja-JP を含み、chunk は 80〜1120 ms、FLEURS の日本語 CER は 80 ms で 13.87、1.12 秒で 11.48）は sherpa-onnx が runtime を取り込んでおり、今の build の `OnlineRecognizer` で測れる見込みである（取り込みが v1.13.8 に入っているかは確かめていない）。Moonshine v0.1.5 の日本語 streaming と Voxtral Realtime（sub-200 ms から設定可）は、sherpa-onnx で動くかを確かめていない。Python や vLLM でしか動かない物は、実行時の Python を 0 本にする方針（1.3）なので採らない。持ち主は R7b（W3 の席。測るだけで、`hayamimi` の経路の code を替えない）で、Nemotron 3.5 の 560 ms の int8 の package を同じ build で読み、lat-ja の `L_asr` の p50 と p95（RP-4 の式）と、eval_real の ja の CER（RP-1 の ja）と golden の 8 本の CER（RP-2）を、今の ReazonSpeech と同じ表に並べて `docs/results/` に書く。Moonshine と Voxtral は、sherpa-onnx の docs と release に在るかだけを確かめて同じ表に書く。置き換えるかは、E2E が通った後に、その表を材料にして評議が決める。いまの W1〜W4 と E2E の経路は替えない。Nemotron 3.5 の license（OpenMDW-1.1）を配る物に入れてよいかは確かめていない（NOT VERIFIED。採る時に R10 が `THIRD_PARTY_NOTICES.md` の手番で確かめる）。
- **H39** 道具の版の差が 3 件ある（同じ文書の表 E）。llama.cpp は b11583 から b11609 へ進んだ。持ち主は R8e で、L を測る前に R8b の置き場の build を b11609 以降の最新に入れ替え、前の版を控えて RP-16 の訳の行と CS-20 を打ち直す。ORT の退路は同じ系列の 1.28.3 にした（3.12）。WSL で退路の build を 1 回作り、RP-17 を通した（6 本 pass、実測 2026-10-14）。xtask の `--ort-version` に退路の既定は無く（既定は現行の 1.31.0）、xtask は変えない。Windows の退路の build は未作成で、持ち主は R10 の続きである（NOT VERIFIED）。現行の 1.31.0 は最新のままである。`requirements.txt` の sherpa-onnx 1.13.6 は上げない。R0 の Python の基準をその版で測ったので、上げると基準を同じ条件で取り直せなくなる。この file は R11 で消す。
- **H41** 同じ process で、sherpa-onnx の CUDA の認識（recognize）の後に `ort` の CUDA で encoder だけの session を開くと、`ort` が落ちる（R7 の計測の道具の実測）。原因は特定していない（NOT VERIFIED）。計測は 2 つを別の process で取る形にした。本番の process も sherpa-onnx の CUDA の session（言語判定）と `ort` の CUDA の session（句読点）を同じ process に開くが、その組は RP-6、RP-16 の実走（T-RP-16-028 の `Engine::open`）で落ちていない。落ちるのが同じ model の file を 2 つの runtime で開く時だけかを、R7 の席が W4 の測り直しの窓で 1 回確かめる（同じ process で、言語判定の CUDA の session の後に句読点の session を開く順と、認識の後に encoder を開く順を 1 回ずつ）。本番の組で落ちれば、3.15 の止めない形に反するので評議へ出す。
- **H36** zh、ko、es ほかの訳の V は、形、数、ラテン文字の名前だけを見て、否定と不確かさを見ない。否定の落ちた訳が V を通りうる（会話字幕の設計 3.4 の否定と推量の表が ja と en にしか無いため）。RP-13 の chrF は否定の落ちを個別には捕まえない。持ち主は R8f で、言語ごとの否定と推量の語の表を足し、ja と en の表と同じ形の case 行（否定を落とした訳が `negation` で止まる）で確かめる。表が入るまで、ja と en の外の言語の訳は、否定の落ちを止められない物として扱う。
V は止めすぎてもいる（RP-13 の run で、正しい訳を止めた行が RP-13 の chrF を下げる）。止めすぎの直しも R8f が持ち、条件は 3 つである（評議の判定）。調整は seed 0 の 50 組とは別の組で行い、seed 0 の 50 組は判定にだけ使う。否定、数、推量を落とした訳が止まる対照の case 行は残す。閾値は緩めない。含意の否定（否定の語を持たない否定の言い方）と伝聞の「そうです」の扱いは、V の規則そのものを変えるので、その数（止めた行と止めなかった行）を添えて評議へ戻す。RP-13 は R11 に入る条件（3.2）に入る。
V の直しの参照訳の止まる行 70→30（seed 0）と 98→55（seed 1）は調整に使った組の値で held-out ではなく、効果は FLEURS の test split の新しい組を 1 回だけ使って出すまで未検証（NOT VERIFIED）、RP-13 は en 以外も含め未通過のまま記録する（評議の判定）。
dev からは新しい組を引けない。dev の ja と対象の言語の重なりは en、zh、ko、es で 135、135、118、135 行で、seed 0 の 50 組を引くと 85、85、68、85 行になり、seed 1 の組と同じ行しか残らないからである。
test split は 321、320、249、320 行で、dev と id の重なりは 0 である。
test split の組は試験の席が引いた（16 file、800 行。置き場は直す席に渡さないので、この文書に書かず state.json に置く）。ids の sha256 の頭 16 字は en `980017baf068f055`、es `e68a64d7cad454db`、ko `af6b2cf34bc5d823`、zh `c25ea26435365121` で、dev との id の重なりは 0 である。打つ前にこの 4 つの値を取り直し、違えば打たない。
V の効果を測る壊した訳の組（`vprobe` の (b)）は、参照訳の数を 1 字ずらす、否定の標識の語を落とす、推量の標識の語を落とす、の 3 つの壊し方で作り、V がその行を止めることを見る。
`fleurs-pairs --split <dev|test>` が、表を引く split を選ぶ（`native/hayamimi-eval/src/fleurs.rs` の `tsv_url`）。
既定は dev で、既存の run の組（`testdata/baseline/pairs`）と byte で一致する。
dev と test 以外の値は使い方の誤り（exit 2）で止まる。
test split の表は `https://huggingface.co/datasets/google/fleurs/resolve/main/data/<config>/test.tsv` にある（T-RP-13-022〜026）。
組を引くのは test-designer の席で、打つのは V を直していない検証の席である。直す席には組の path を渡さず、組は 1 回打ったら使わない。
品質の記録（RB-7 の値）は書き換えない。
E を掛けない腕で出た en の 46.65 は RP-13 の en の値ではなく、RP-13 の en は未測定である。
V の直しは `d888f87` の 1 commit で、revert で戻せる。
T-RP-13-012、013、016、017、018 は、ja と en の外の言語で E を掛けず、V で外れた行を `held` にする形に合わせて書き替える。書き替えは test-designer の席へ回す（セラが運ぶ）。
- **H33** CPU に落ちた段の誤り率は、同じ model を読むので GPU の run と同じと見込んでいる（ASSUMPTION）。fp32 の数値の差で字が変わりうるので、R2 が RP-18 の句読点の全件を CPU の provider でも掛け、`expected` と一致する件数を記録する。小さい model へ落ちた段の誤り率は測っていない（NOT VERIFIED）。
- **H42** Python の `final` は `t1_s` を持たないので、Python の run の遅れて着いた `final` は、前の窓が「final なし」で次の窓に `final` が 2 件以上在る形からの推定でしか数えられない（3.1）。推定の数が Python の p95 を小さく見せている量は分からない（NOT VERIFIED）。持ち主は R8h で、RP-32 の Python の run でその数を記録し、Rust の同じ数（`t1_s` で結んだ時に次の窓に着いていた `final` の数）と並べる。
- **H43** R8e の run の「訳」の段と `decode_ms` の差と、3.1 の数え方で数え直した `L`（p50 1533 ms、p95 2752 ms、母数 67。数え直す前より下がる）は、原因の席と試験の席の打ち直しの値で、設計の席は確かめていない（NOT VERIFIED）。数え直すと母数が 93 から 67 に減り、訳の取れた割合が RP-32 の 20 / 23 に届かない。`decode_ms` の差が負荷の差か engine の差かも分かっていない。Python の `decode_ms` が何を測った値か（待ちを含むか）と、発話の終わりの検出（`detected()`）の挙動も確かめていない。3.16 の 2 の A で、2 本並べた依頼が T の待ちを延ばす量も測っていない。持ち主は R8h で、RP-32 と RP-34 の run の段ごとの列で確かめ、2 の A を作るかの材料にする。

## 11. 出所

取得日は S19 と S20 が 2026-10-14、ほかはすべて 2026-10-13。
crate の版は crates.io の API（`https://crates.io/api/v1/crates/<名前>`）で引いた。
S12 と S15 は別の席の調べの転記で、この席は原典を開いていない。

| 番号 | 事実 | 出所 |
|---|---|---|
| S1 | crate の版：`sherpa-onnx` 1.13.8（feature は `static` と `shared`、既定は `static`）、`ort` 2.0.0-rc.14（`cuda`、`tensorrt`、`load-dynamic` ほか）、`wasapi` 0.26.0、`cpal` 0.18.2、`dioxus` 0.7.10、`axum` 0.8.9、`tokio` 1.53.2、`ct2rs` 0.10.1（採らない）、`vibrato` 0.5.2 と `lindera` 6.2.0（採らない）、`kiwi-rs` 2026.8.26、`ferrous-opencc` 0.4.0（Apache-2.0、OpenCC の Rust 実装）、`symphonia` 0.6.1、`rubato` 5.0.1。`sherpa-onnx` の config の型が `provider` を持つこと | https://crates.io/api/v1/crates/sherpa-onnx 、https://docs.rs/sherpa-onnx/1.13.8/sherpa_onnx/all.html 、https://docs.rs/sherpa-onnx/1.13.8/sherpa_onnx/struct.OfflineModelConfig.html |
| S2 | sherpa-onnx の `asr-models` の release の file 名（fp32 と int8 の版の有無） | https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/asr-models |
| S3 | `sherpa-onnx-sys` 1.13.8 の build script は `SHERPA_ONNX_LIB_DIR` を読み、在ればその directory に繋ぐ。CUDA 版を自分では取らない。v1.13.8（2026-09-10）の release の CUDA 版の名前は `cuda-13.x-cudnn-9.x-onnxruntime1.28.2` ほか | https://docs.rs/crate/sherpa-onnx-sys/1.13.8/source/build.rs 、https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/v1.13.8 |
| S4 | `ort` の `IoBinding`：`bind_output_to_device` で出力を device に置ける。出力は `synchronize_outputs` まで同期されない | https://ort.pyke.io/perf/io-binding |
| S5 | ORT の CUDA 実行 provider：`user_compute_stream`、`enable_cuda_graph` の制約、1.27 以降の既定の build は CUDA 13.0 と cuDNN 9 | https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html |
| S6 | ORT の op の表：CUDA の節に `MatMulInteger` は在り、`DynamicQuantizeLinear`、`DynamicQuantizeMatMul`、`STFT`、`DFT` は無い | https://github.com/microsoft/onnxruntime/blob/main/docs/OperatorKernels.md |
| S7 | `wasapi` crate は loopback capture、shared と exclusive、event 駆動を持つ | https://docs.rs/wasapi/latest/wasapi/ |
| S8 | Silero VAD：30 ms 以上の塊が CPU の 1 thread で 1 ms 未満、batch か GPU でさらに速くできる | https://github.com/snakers4/silero-vad |
| S9 | IWSLT 2026 の cascade の同時通訳の system（文字単位の最長共通接頭辞の re-translation、AlignAtt4LLM、NeMo の transducer と LLM） | https://aclanthology.org/2026.iwslt-1.32.pdf 、https://aclanthology.org/2026.iwslt-1.23.pdf 、https://www.alphaxiv.org/abs/2606.03967.md |
| S10 | ORT v1.28.0 で sm_120 の GroupQueryAttention の不具合を直した。CUDA の Plugin EP v0.1.0 が SM120 の build を改善した | https://newreleases.io/project/github/microsoft/onnxruntime/release/v1.28.0 、https://newreleases.io/project/github/microsoft/onnxruntime/release/plugin-ep-cuda%2Fv0.1.0 |
| S11 | sherpa-onnx の C の API：`SherpaOnnxAcceptWaveformOffline(stream, sample_rate, const float *samples, n)`、`SherpaOnnxVoiceActivityDetectorAcceptWaveform(p, const float *samples, n)`、`SherpaOnnxOfflineModelConfig` の `provider`（`"cpu"` か `"cuda"`） | https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/sherpa-onnx/c-api/c-api.h |
| S12 | sherpa-onnx の issue 878：TTS で CPU の方が速く、保守者は GPU の warmup と転送の手間を挙げた | 別の席の転記。https://github.com/k2-fsa/sherpa-onnx/issues/878 |
| S13 | sherpa-onnx の GPU の build：`cmake -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON -DSHERPA_ONNX_ENABLE_GPU=ON ..`、事前に入れた ORT は `SHERPA_ONNXRUNTIME_LIB_DIR` と `SHERPA_ONNXRUNTIME_INCLUDE_DIR` で渡す | https://k2-fsa.github.io/sherpa/onnx/install/linux.html |
| S14 | llama-server の `/v1/chat/completions` は、model の metadata の chat template を当てて message を model に渡す（`--chat-template` の既定は model の metadata、`--jinja` は既定で有効）。`/completion` は `prompt` に文字列か token の id を受け、`stop` と `cache_prompt` を受け、`/completion` に固有の項目は chat の endpoint でも受けると書かれている | https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md |
| S15 | 別の席の調べの転記：llama.cpp の release に win の CUDA 13 の build、`ort` の配布の binary は CUDA 13.2 以上と cuDNN 9.23 以上、TensorRT の初回は cache なしで 384 秒、end-to-end の model（Whisper translate、SeamlessStreaming、Canary、Parakeet、Riva）が使えない理由 | web-researcher の席の報告（2026-10-13、セラ経由の転送） |
| S16 | sherpa-onnx v1.13.8 の cmake：`onnxruntime-win-x64-gpu.cmake` の 22 行目が「Requires cuda 12.x, cudnn 9.x」、23 行目が Microsoft の `onnxruntime-win-x64-gpu_cuda12-1.28.2.zip` を指す。`onnxruntime-linux-x86_64-gpu.cmake` の 22 行目が「Requires CUDA 12, cudnn 9」、23 行目が保守者の `onnxruntime-linux-x64-gpu_cuda12-1.28.2-patched.zip` を指す。`onnxruntime.cmake` の 156〜178 行目は、`SHERPA_ONNXRUNTIME_INCLUDE_DIR` と `SHERPA_ONNXRUNTIME_LIB_DIR` が在ればその `onnxruntime.dll` と `onnxruntime.lib` を使う。ORT v1.28.2 の Windows の GPU の asset は `onnxruntime-win-x64-gpu_cuda12-1.28.2.zip` と `onnxruntime-win-x64-gpu_cuda13-1.28.2.zip` の 2 つ。保守者の `csukuangfj/onnxruntime-libs` の v1.28.2 に `onnxruntime-linux-x64-gpu_cuda13-1.28.2-patched.zip` が在る | https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/cmake/onnxruntime-win-x64-gpu.cmake 、https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/cmake/onnxruntime-linux-x86_64-gpu.cmake 、https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/cmake/onnxruntime.cmake 、https://api.github.com/repos/microsoft/onnxruntime/releases/tags/v1.28.2 、https://api.github.com/repos/csukuangfj/onnxruntime-libs/releases/tags/v1.28.2 |
| S17 | llama.cpp の release b11583（2026-10-13T09:34Z）の Windows の CUDA の build は `llama-b11583-bin-win-cuda-12.4-x64.zip`、`llama-b11583-bin-win-cuda-13.4-x64.zip`、`cudart-llama-bin-win-cuda-12.4-x64.zip`、`cudart-llama-bin-win-cuda-13.4-x64.zip` ほか。`sm_120` を持つ最初の CUDA が 12.8 であることは NVIDIA の CUDA 12.8 の release notes による（この席は原典を開いていない。転記） | https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=5 |
| S18 | CUDA の JIT の cache：`CUDA_CACHE_PATH` が cache の directory を決め（既定は Linux が `~/.nv/ComputeCache`、Windows が `%APPDATA%\NVIDIA\ComputeCache`）、`CUDA_CACHE_DISABLE=1` で cache を使わない。埋め込みの機械語が当たらない kernel は PTX から変換される | https://docs.nvidia.com/cuda/cuda-programming-guide/05-appendices/environment-variables.html |
| S19 | ORT v1.31.0（2026-10-09 公開）の CUDA 13 版の asset：`onnxruntime-win-x64-gpu_cuda13-1.31.0.zip`（309,484,217 bytes、sha256 `d332ccf8…552f5`）、`onnxruntime-linux-x64-gpu_cuda13-1.31.0.tgz`（244,574,938 bytes、sha256 `2867dec9…81135`） | https://api.github.com/repos/microsoft/onnxruntime/releases/tags/v1.31.0 |
| S20 | sherpa-onnx v1.13.8 の FastClustering は `Eigen/Dense` と `fastcluster-all-in-one.h` だけを include し、`fastclustercpp::hclust_fast` の complete linkage で群を作る。ORT と CUDA を使わない | https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/sherpa-onnx/csrc/fast-clustering.cc |
