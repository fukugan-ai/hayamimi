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
- CPU の翻訳器（FuguMT、M2M-100）の Rust 版。訳は GPU の言語モデル（llama-server）だけで行う（3.6）。
- mobile/（Flutter）の作り直し。wire 形式の互換だけを守る（3.8）。
- GPU の無い機械で動く道。GPU が無ければ、理由を出して起動を止める（3.11）。
- 精度か速度を今より落とした状態で「置き換えた」とすること。

### 1.4 GPU の既定

全段を GPU に載せるのを既定とする。
CPU に残す段は、段ごとに「GPU の方が遅いという実測」か「GPU へ移して縮む時間が `L` に対して無視できるという実測」か「GPU で動く経路が無いという根拠」を 3.5 の表に持つ。
根拠の無い段を CPU に残さない。
GPU と CPU のどちらが速いかの実測は、R1（計測の slice）で GPU の経路が在る段について取る。
GPU の経路が無い 5 段（取り込み、ITN、句読点の後処理、清書の clustering、配信）は、その段を作る slice が CPU の時間を測る（3.5）。

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
| 会話字幕の設計 `conversation_subtitles.ja.md` | 認識を Python に残す決め、chrF を Rust で書かない決め、V を 1 回とする決め、llama-server を WSL に置く決め、gnu の target、`<relay>` の path、CS-3 と CS-20 | この文書と同じ手番で書き替えた。残る 1 件（slice 1 の case 行 T-CX-1-002）は R8c で test-designer が書き替える（会話字幕の H16） |
| `docs/design/goals.md` | 実行環境の行、GPU の名前、候補アプローチの見出し、残課題の GPU の行 | この文書と同じ手番で書き替えた |
| `README.md` の 5 行目、`README.ja.md` の 5 行目と 15 行目（CPU だけで動くという売り） | 3 行 | R10 で `readme-writing.md` の工程で書き替える |
| `state.json` の `constraints` の「Rust への移植は速度が足りないと測れたときだけ」 | 1 行 | この文書の目標と食い違う。state.json を書く席（セラ）が直す（10 の H22） |

### 2.5 今の速さの記録

`docs/results/benchmarks.md` と `goals.md` に在る確定の遅れは平均と最大だけで、p50 と p95 は無い（`grep -n -iE 'p95|p50|median'` が 0 件、同じ file の `平均` は複数件）。
記録は「ja 確定 平均 98〜103 ms（句読点込み）」「90 秒の 5 言語 平均 236 ms、最大 552 ms」で、この ms は VAD が区切りを決めてから確定文が出るまでである（`realtime_transcribe.py` の `drain_segments` が `vad.front` を取った時刻から測る）。
VAD が発話の終わりを決めるまでの無音待ち（`--min-silence` 0.35 秒）は含まない。
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
| `t_tr` | その発話の訳の event のうち、訳の元にした原文の文字列がその発話の `final` の文字列と一致する物を、道具が初めて受けた時刻 |
| `t_partial` | その発話の最初の `partial` event を道具が受けた時刻 |

`L = t_tr − t_end`、`L_asr = t_final − t_end`、`L_tr = t_tr − t_final` とする。
`t_tr` を「原文が `final` と一致した訳」に限るのは、速報の断片の訳で `L` が小さく、負にもなり、受入が中身の無いまま通るのを防ぐためである。
中継は訳の event に、訳の元にした原文の文字列の sha256 を `src_sha256` として載せ、道具は `final` の文字列の sha256 と比べる。
Python の `translation` event は `src_sha256` も `id` も持たないので、Python の run は「`final` の後、次の `final` までに最初に来た訳」で対応づけ、訳の取れない発話の数を別に記録する（`rust_port_tests.ja.md` の RB-5）。
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
2. RP-1、RP-14、RP-15 が `<base>` の値と照らして通っている。
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
3. **縮む時間の上限**：CPU の p50 が下限より大きくても、GPU へ移して縮む時間は CPU の p50 を超えない。その上限が `L` の見積りの p50 の 1% より小さければ CPU に置く（1% は仮の値。10 の H8）。
4. **GPU の経路なし**：GPU で動く経路が無い段は「CPU（GPU 経路なし）」と書き、その段を作る slice が CPU の p50 を `hayamimi-eval stages` で測って `gpu_stages.md` の同じ段の CPU の行に置く。受ける式は「CPU の p50 が GPU の往復の下限より小さい」で、成り立たなければその slice が評議へ戻す。

**GPU の往復の下限**は、`gpu_stages.md` の「認識の文字列 → 訳の入力」の GPU の行の無負荷の 3 run の p50 の最小で、いまは 219 µs（3 run は 219〜232 µs、`gpu_stages.md:184-186`）である。
根拠の列の `gpu_stages.md` の行番号は、段の名前の行（「段ごとの時間」の表）と置き場の提案の行（「置き場の提案」の表）を指す。

| 段 | いまの置き場 | 根拠、または R1 で測る物 |
|---|---|---|
| 音声の取り込み（WASAPI のマイク、loopback） | CPU（GPU 経路なし） | 根拠 4。WASAPI の capture は host の buffer に sample を渡す（S7、https://docs.rs/wasapi/latest/wasapi/ ）。R5 が、取り込みから認識器へ渡すまでの CPU の p50 を測る |
| int16 → float32、16 kHz への resample、mix | CPU | 根拠 3。CPU の p50 は無負荷 2455 µs、負荷の下 3195 µs で、GPU の行は kernel を含まない下限の 345 µs と 368 µs である（`gpu_stages.md` の「int16 → float32、16 kHz への resample、mix」の行、:27-35、:248）。GPU へ移して縮む時間の上限 2455 µs は、`L` の見積りの p50 1349 ms（「遅れ L の見積り」の「Rust 認識 cuda / 訳 cuda 無負荷」の行、:292）の 0.2% である（負荷の下も 3195 µs 対 1712 ms で 0.2%、:294）。sherpa-onnx の C の API は音声を host の `const float *` で受ける（S11） |
| 音声の ring buffer と preroll（1.0 秒） | CPU | 根拠 2。CPU の p50 15〜22 µs が、GPU の下限 247〜267 µs（無負荷）より小さい（「音声の ring buffer と preroll」の行、:36-44、:249）。認識器へは host の配列で渡す（S11） |
| VAD（Silero、sherpa-onnx の `provider`） | CPU | 根拠 1（variant `fp32`）。同じ fp32 で、無負荷の p50 が GPU 388 ms に対し CPU 40 ms、負荷の下は GPU 1121 ms に対し CPU 42 ms（「VAD」の行、:45-53、:250） |
| 言語判定（whisper-tiny） | GPU | 根拠 1（variant `fp32`）。無負荷の p50 が GPU（fp32）76 ms に対し、CPU は fp32 で 132〜152 ms、int8 で 124 ms。負荷の下は GPU 94 ms に対し CPU の int8 132 ms（「言語判定」の行、:54-66、:251）。CUDA の session でも CPU に置かれる node は tiny-encoder が 31、tiny-decoder が 201 で（:240-241）、上の GPU の値はそれを含む |
| 認識の encoder（全 model） | CPU（int8） | 根拠 1（variant `fp32`）。sherpa-onnx の認識全体（encoder と探索）の同じ fp32 の対で、無負荷の p50 が GPU 171414 µs に対し CPU 151299 µs、負荷の下は GPU 375176 µs に対し CPU 175025 µs（「認識の encoder」の行、:67-94、:252）。CPU の中では int8 が 128615 µs で fp32 より速いので int8 を読む（int8 は CUDA に op が無く、置き場の比較には入れない。3.4）。encoder だけを `ort` で掛けると GPU 66262 µs に対し CPU 121091 µs で GPU が速いが、provider は認識器の config に 1 つで encoder と探索を別の provider に分けられない（S11 の `SherpaOnnxOfflineModelConfig`）。この置き場は測った ReazonSpeech の transducer に限る。他の認識器は R3 が同じ式で測る。GPU と CPU の差は 2〜9% と小さく、CPU の負荷が重なると CPU の側が悪化する。R7 の 2 音源同時で測り直す |
| fbank と探索（sherpa-onnx の中） | sherpa-onnx の実装のまま（host） | provider に依らず host で走る（「fbank と探索」の行、:95-112、:253）。この行は fbank だけで、探索は認識の encoder の行の合算に入っている。探索の時間は、合算から encoder だけの時間を引いて出す（10 の H11、H31） |
| 冒頭欠落の疑い判定と分割再試行 | CPU（判定）。解き直しは認識の encoder の置き場に従う | 根拠 2。判定の CPU の p50 1 µs が、GPU の下限 216〜241 µs（無負荷）より小さい（「冒頭欠落の疑い判定と分割再試行」の行、:113-121、:254） |
| ITN（漢数字 → 算用数字） | CPU（GPU 経路なし） | 根拠 4。model を持たない文字列の規則（正規表現）で、ORT も CUDA も呼ばない。R2 が 1 行の CPU の p50 を測る |
| 句読点（BERT、`ort` の CUDA） | GPU | 根拠 1（variant `fp32`）。`ort` で `punct_ja_parity.json` の `input_ids` を forward だけ掛けた同じ fp32 の対で、無負荷の p50 が GPU 8272 µs に対し CPU 44899 µs、負荷の下は GPU 11112 µs に対し CPU 41557 µs（「句読点」の行、:126-135、:256）。GPU の悪い方の run の 15398 µs（:130）も、CPU の速い方の run の 40110 µs（:135）より速い。CUDA の session でも CPU に置かれる node が 77 在り（:239）、上の GPU の値はそれを含む。入力の文字列 → token id の変換は表を引く 1 回で、CPU で行う |
| 句読点の後処理と疑問符の規則、置換辞書 | CPU（GPU 経路なし） | 根拠 4。model を持たない文字列の規則で、ORT も CUDA も呼ばない。R2 が 1 行の CPU の p50 を測る |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | CPU | 根拠 1（variant `fp32`）。同じ fp32 で、無負荷の p50 が GPU 227 ms に対し CPU 71 ms、負荷の下は GPU 268 ms に対し CPU 83 ms（「話者の埋め込み」の行、:140-148、:258） |
| 話者の速報の割り当て（cosine と重心の更新） | CPU | 根拠 2。CPU の p50 2〜3 µs が、GPU の下限 237〜251 µs（無負荷）より小さい（「話者の速報の割り当て」の行、:149-157、:259） |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | CPU | 根拠 1（variant `fp32`）。同じ fp32 で、無負荷の p50 が GPU 118 ms に対し CPU 34〜39 ms、負荷の下は GPU 205 ms に対し CPU 41〜42 ms（「清書の再分離」の行、:158-170、:260） |
| 清書の clustering（FastClustering）と全体の対応づけ | CPU（GPU 経路なし） | 根拠 4。sherpa-onnx の FastClustering は Eigen と fastcluster の C++ で、ORT も CUDA も使わない（S20）。再分離と同じ呼び出しの中で走り、分けて測る口が無い（「清書の clustering」の行、:171-174）。R4 が、FastClustering と全体の対応づけの CPU の p50 を測る |
| 訳 T と E（llama-server、CUDA） | GPU | 根拠 1（variant `CAT-Translate-3.3b-Q8_0`）。T の無負荷の p50 が GPU 253 ms に対し CPU 2858 ms、負荷の下は GPU 435 ms に対し CPU 3337 ms（「訳 T と E」の行、:175-183、:262）。測ったのは WSL の CUDA 12.0 の build の llama-server で、本番の Windows の CUDA 13.4 の build ではない。R8b が Windows の CUDA 13.4 で T を測り直し、`gpu_stages.md` の T の行を置き換える。E は測っていない |
| 認識の文字列 → 訳の入力 | CPU（文字列） | 根拠 2。CPU の p50 2 µs が、GPU の下限 219〜232 µs（無負荷）より小さい（「認識の文字列 → 訳の入力」の行、:184-192、:263）。認識の token と言語モデルの token は語彙が違い、文字列に戻して切り直すしかない。llama-server の API は文字列か token の id を受ける（S14） |
| 照合 V と単位の換算 | CPU | 根拠 2。CPU の p50 8〜9 µs が、GPU の下限 220〜251 µs（無負荷）より小さい（「照合 V と単位の換算」の行、:193-201、:264）。会話字幕の CX-3 と CS-6 の 1 行あたり 5 ms 以下も満たす |
| 字幕の配信（SSE）と描画 | CPU（GPU 経路なし）。描画は WebView2 | 根拠 4。文字列を SSE で送る段で、GPU で動く経路は無い。R5 が、確定文の event を `/events` へ書き出すまでの CPU の p50 を測る。描画の時間は会話字幕の CS-7 の中で測る |

段と段の間の転送は、R1 で発話ごとに測る。
sherpa-onnx の中の転送は、CUDA の profiler（Nsight Systems の `nsys profile --stats=true` の `cuda_gpu_mem_time_sum` と `cuda_gpu_mem_size_sum`）で、1 発話あたりの host → device と device → host の回数、bytes、時間を数える。
profiler が Windows で使えないときは WSL の同じ build で数え、そのことを結果の file に書く。

### 3.6 訳の速度

会話字幕の訳は T（訳す）→ E（自然に直す）→ V（照合）の 3 段で、T と E は言語モデルに 1 回ずつ頼む。
試し 1 の実測では、CAT-Translate-3.3b の 1 回が中央値 148 ms、p95 277 ms だった（`conversation_subtitles.ja.md` 2.3）。
T と E を直列に待つと、訳の遅れはその 2 倍になる。
速度を第一にするので、次の 6 つで `L_tr` を縮める。

1. **先に T の訳を出す**：T の応答に V を掛け、通れば先の訳として自分の重ね窓と OBS の字幕頁に出す（`status: "ok"`、`stage: "t"`）。続けて E を頼み、E の応答に V を掛けて通れば同じ `id` の行を置き換える（`stage: "e"`）。E が V に落ちたら T の訳を残す。V は 1 行につき多くて 2 回で、同じ訳には 2 回掛けない。
2. **確定の訳だけを外へ出す**：**確定の訳**は、E の訳が V を通ればその訳、外れて T の訳が通っていれば T の訳とし、E の応答を照合した後に 1 回だけ決める。Zoom と Teams への送信と SRT は確定の訳だけを使い、1 つの `id` につき 1 回送る（会話字幕の設計 3.4、3.10、3.13）。送った字幕は相手の画面で置き換えられないからである。
3. **速報（partial）の訳**：認識の速報が更新されるたびに、その文字列を T に頼み直す（re-translation）。前の訳と今の訳の最長共通接頭辞だけを確定した字形で描き、残りは薄く描く。IWSLT 2026 の Pinch-AST が同じ文字単位の最長共通接頭辞の方針を取っている（S9）。速報の依頼は 1 本だけを走らせ、新しい速報が来たら走っている依頼を打ち切る（stream の応答の接続を閉じる。llama-server が接続を閉じたときに生成を止めるかは 10 の H6）。`cache_prompt` を真にし、伸びた文字列の前の部分の計算を使い回す。速報の訳にも V を掛け、外れたら速報の訳は出さない。
4. **揺れを数える**：1 つの発話について、字幕に出た訳の文字列が変わった回数と、一度確定した字形で描いた接頭辞が後で消えた回数を数える（RP-25）。速度のために先に出す形が、読む人に揺れとして見える量を測るためである。
5. **並べて走らせる物**：llama-server の slot を 4 にする（mic の確定、loopback の確定、速報、E）。認識（ORT）と訳（llama.cpp）は同じ GPU を時間で分け合う。負荷の下での `L` は RP-3 で測る。
6. **llama-server の置き場**：Windows の native の CUDA 版に置く（会話字幕の設計 3.7）。model の GGUF は `%LOCALAPPDATA%\hayamimi\models\llm\` に置き、正本の WSL の `/mnt/h3-models` から写す（2.3。Windows から直に読めないため）。写した file の sha256 を同じ directory の `manifest.json` に記録する（記録で、起動の条件にしない）。hooks の `guard_commands.rs` は、`llama-` で始まる実行 file の名前を含む command を WSL の build の鮮度で refuse する（`problems` の中の `LLAMA_EXECUTABLE` の検査）。Windows の llama-server は PowerShell から起こし、WSL の shell からは起こさない（10 の H20）。

`--translate` の言語（今は FuguMT の en と、M2M-100 の zh、ko、es ほか）も、同じ言語モデルの T で訳す。
品質は FLEURS の chrF で、今の値（en 47.57、zh 20.19、ko 25.20、es 42.09）以上を RP-13 で測る。
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

### 3.11 GPU の無い機械

起動時に CUDA の device を数え、0 なら model を 1 つも読まずに止まる。

- `gpu.device.unavailable` を error の log の event で 1 回出す（`error_code` は `HAYAMIMI_GPU_DEVICE_UNAVAILABLE`）。
- 利用者の locale の文で、理由（CUDA の device が見つからない、driver か CUDA の runtime が読めない、のどちらか）と、確かめる command（`nvidia-smi`）を stderr に出す。
- 終了 code は 0 以外にする。

CPU に落として動かす道は作らない。
CPU で動かす道を残すと、GPU を前提にした速度の受入が測らない経路が 1 本増え、その経路の誤り率と速さを別に保つ手間が掛かるからである。
訳の中継（`hayamimi-relay`）は今のまま、llama-server に届かなければ `hold_reason: unavailable` を出す（会話字幕の設計 3.6）。

### 3.12 CUDA 版の sherpa-onnx の build

手順書は `docs/guide/build_gpu.ja.md`（R1 の席が書く）に置き、手順は `cargo run -p xtask -- sherpa-gpu` の 1 本で打てる形にする。
xtask は次の版を引数の既定に持ち、build の後に実際の版を `native/target/sherpa-gpu/<os>/build.json` に書く。
版は記録で、build の可否の条件にしない（外が更新する物の版を gate にしない）。

| 項目 | 既定 | 根拠 |
|---|---|---|
| sherpa-onnx | tag `v1.13.8`（Rust crate の版と同じ。crate の build script が同じ版の C の API を前提にする） | S3 |
| ONNX Runtime | 1.31.0 の CUDA 13 版。両 OS とも Microsoft の release の asset で、Windows は `onnxruntime-win-x64-gpu_cuda13-1.31.0.zip`（309,484,217 bytes、sha256 `d332ccf8…552f5`）、WSL は `onnxruntime-linux-x64-gpu_cuda13-1.31.0.tgz`（244,574,938 bytes、sha256 `2867dec9…81135`）。bytes と sha256 は記録で、取得の可否の条件にしない。展開した directory を `SHERPA_ONNXRUNTIME_INCLUDE_DIR` と `SHERPA_ONNXRUNTIME_LIB_DIR` で cmake に渡す。1.28.2 の CUDA 13 版は退路で、その build を `native/target/sherpa-gpu-ort1282/` に残す（下の戻し方） | S16、S19 |
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
退路の ORT v1.28.2 の CUDA 13 版の zip の `onnxruntime_providers_cuda.dll` を、CUDA 13.4 の `cuobjdump` で数えた（実測、2026-10-13）。
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
| Windows | ORT 1.28.2 の CUDA 13 版（退路） | build と install を `native/target/sherpa-gpu-ort1282/` に残す | 退路が要らなくなったら、その directory を消す |
| Windows と WSL | 退路と再現の build | `native/target/sherpa-gpu/` は 1.31.0 と CUDA 13.4.3 の現行、`sherpa-gpu-repeat/` は同じ版の 2 回目の build（RP-24 の陽性対照）、`sherpa-gpu-cuda1341/` は CUDA 13.4.1 の build（CUDA の退路） | 要らなくなった directory を消す。現行の `sherpa-gpu/` は消さない |
| WSL | WSL の build の置き場 | build の本体は `~/.local/share/hayamimi/build/` の下の 3 本である。現行（CUDA 13.4.3）の `cuda1343` と、その 2 回目の `cuda1343-repeat` は、どちらも `linux`、`ort`、`src-v1.13.8`、`downloads` を持つ。CUDA 13.4.1 の build の `cuda1341` は、その build の `linux` の中身（`build.json`、`install`、`rp17_passed.json`）を directory の直下に持つ（`build.json` の `cuda_version` は 13.4.1 の nvcc の版の 13.4.59）。`native/target/sherpa-gpu/linux`、`sherpa-gpu-repeat/linux`、`sherpa-gpu-cuda1341/linux` は、それぞれ `~/.local/share/hayamimi/build/cuda1343/linux`、`cuda1343-repeat/linux`、`cuda1341` を指す symlink である。`~/.cache/hayamimi-r1-wsl-20261014` に残っているのは、全 build 共通の cargo の `target`（作り直せる cache）、取得した archive（`downloads`、`redist`、`redist3`）、13.4.1 の build の `ort` と `src-v1.13.8` である。13.4.1 の 2 回目の `~/.cache/hayamimi-r1-wsl-20261014-repeat`（`sherpa-gpu-cuda1341-repeat/linux` が指す）も `~/.cache` に在る。build の本体を `~/.cache` の外に置くのは、`~/.cache` が掃除の対象になる置き場で、消えると現行の build が切れるからである。Windows からは WSL が作った symlink を辿れない（RP-24 の os error 1920） | 戻し方は `mv` で `cuda1343` を `~/.cache/hayamimi-r1-wsl-20261014-cuda1343` へ、`cuda1343-repeat` を `~/.cache/hayamimi-r1-wsl-20261014-cuda1343-repeat` へ、`cuda1341` を `~/.cache/hayamimi-r1-wsl-20261014/linux` へ戻し、`ln -sfn` で 3 本の symlink をそれぞれの `linux` へ張り直す。戻すのに消す操作は 0 である。消し方は、現行の `cuda1343` を消さない。`cuda1343-repeat` と `cuda1341` は要らなくなったら directory ごと消し、指していた symlink も消す。`~/.cache` に残した `target` と archive を残すかは R1b の評議が決める |
| Windows | CUDA toolkit 12.9.1 と cuDNN の CUDA 12 版（入っていれば） | 使わない | R1 の最初の手番で、CUDA 13 版の ORT で sherpa-onnx が build でき RP-17 が通ったら、Windows の「アプリ」から 12.9 の部品だけを外し、cuDNN の CUDA 12 版の展開先を消す |
| Windows | CMake、Ninja | 4.4.4、1.13.2、入っている | `winget uninstall` で外す |
| Windows | rustup（msvc の toolchain） | 1.29.1、入っている | `winget uninstall` で外す（`%USERPROFILE%\.rustup` と `%USERPROFILE%\.cargo` も消える） |
| Windows | Git | 2.55、入っている | `winget uninstall` で外す |
| WSL | CUDA toolkit 13.4.3 | redistrib を 13.4.1 の `/usr/local/cuda-13.4` の横に展開済み（9 部品の sha256 が redistrib の json と一致）。13.4.1（退路）と 13.3 は消さずに並べ、`/usr/local/cuda` は 13.3 を指したまま。xtask には `--cuda-dir` で 13.4.3 の directory を渡す | 展開した 13.4.3 の directory を消す。退路の 13.4.1 が要らなくなったら `/usr/local/cuda-13.4` を消す（外す設定は無い。手順は `docs/guide/build_gpu.ja.md`） |
| WSL | cuDNN 9（CUDA 13 版） | 9.27.0.42 を NVIDIA の redistrib から `/usr/local/cudnn-9.27.0.42-cuda13` に展開済み（redistrib の json の sha256 と照合） | `/usr/local/cudnn-9.27.0.42-cuda13` を消す（手順は `docs/guide/build_gpu.ja.md`） |

入れた物の版は `build.json` に記録する。
戻した後に何も残っていないかは、`winget list` と Windows の「アプリ」に上の名前が出ないことで確かめる。
12.9 の部品を外した後に、13.4 の `nvcc --version` と `nvidia-smi` が外す前と同じ出力であることも同じ手番で見る。

**ORT 1.31.0 が動かないときの戻し方**：どちらかの OS で ORT 1.31.0 の build か RP-17 が落ちたら、2 つの OS を両方とも退路の ORT 1.28.2 の CUDA 13 版に戻す（Windows は `onnxruntime-win-x64-gpu_cuda13-1.28.2.zip`、WSL は保守者の `onnxruntime-linux-x64-gpu_cuda13-1.28.2-patched.zip`）。
Windows では `SHERPA_ONNX_LIB_DIR` を `native/target/sherpa-gpu-ort1282/` の install の `lib` に替えて繋ぎ直す。WSL では xtask に `--ort-version 1.28.2` と `--out-dir native/target/sherpa-gpu-ort1282` を渡して build する。
CUDA 13.4.3 の側だけが落ちたときは、CUDA の退路の `native/target/sherpa-gpu-cuda1341/`（13.4.1 の build）に同じ形で繋ぎ直す。
片方だけを戻すと、RP-24 が ORT の版の不一致で落ちるためである。

**CUDA 13 版の ORT が動かないときの戻し方**：1.28.2 の CUDA 13 版でも sherpa-onnx の build か RP-17 が通らなければ、12.9.1 は外さない。
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
| `gpu.session.created` | session を作るたび | model の名前、要求した provider、実際に使った provider |
| `gpu.session.placed` | `--gpu-diagnose` で起こしたとき | ORT が CUDA に置いた node の数と CPU に置いた node の数 |
| `gpu.device.unavailable` | 起動時に CUDA の device が 0 | `error_code` |

本文、音声、path は log に書かない。

### 3.14 言語の切り替え（i18n）

log の `message`、error、CLI の使い方、画面の文言は、locale で切り替える。
locale は OS の値（Windows は利用者の UI の言語、Linux は `LANG` と `LC_ALL`）を `sys-locale` crate で 1 回読み、既定は英語とする。
locale は実行の環境が意味を決める値で、この repo の設定ではないので、起動時に 1 回読む（`rust_guide_for_agents.md` の環境変数の節の範囲の外）。
文言は `hayamimi-relay` と同じ形の `strings.<locale>.toml`（en と ja）に置き、code に自然言語を直書きしない。
`hayamimi-ui` の画面の文言も同じ表から取る。
字幕の event の JSON の key と値（`final`、`lang` ほか）は wire 形式なので切り替えない。

## 4. 採らない手段

- **Python を残して Rust から呼ぶ**：実行時の経路に Python が残り、置き換えにならない。
- **クラウドの認識と翻訳の API**：会話の音声と文字を外へ出す。
- **CPU の翻訳器の Rust 版（ct2rs で FuguMT と M2M-100）**：訳の質の上限が model にあり（`translate.md`、`translate_m2m.md` の失敗例）、GPU の言語モデルで今以上の chrF を測れる（RP-13）。
- **sherpa-onnx の配布の CUDA 版の archive をそのまま使う**：`sm_120` 系の判定（3.12）の材料になる機械語と PTX の件数と、ORT と CUDA の版を自分で固定できない。自分で build して `build.json` に記録する（3.12）。
- **CUDA 12.9.1 と cuDNN の CUDA 12 版を並べて入れ、sherpa-onnx の既定の CUDA 12 版の ORT を使う**：ORT 1.31.0 に CUDA 13 の公式の build が在り（退路の 1.28.2 にも在る）、環境変数 2 つで sherpa-onnx に渡せる。CUDA を 13.4 の 1 本にすれば、入れて保つ版が 1 つ減り、llama-server とも揃う。CUDA 13 版が動かないときの戻し先としてだけ残す（3.12）。
- **ORT を CUDA 13 で source から build し直す**：公式の CUDA 13 版の build が在り、手数を掛ける理由が無い。
- **llama.cpp を CUDA 12.4 の build にする**：12.4 は `sm_120` を持たず、認識の側の 13.4 と版が割れる（3.12）。
- **GPU の無い機械で CPU に落として動かす**：3.11。
- **int8 の model を CUDA で動かす**：動的量子化の op が CUDA に無く、node が CPU に落ちる（3.4）。
- **形態素解析の crate（vibrato、lindera）を入れる**：句読点の token 化は「NFKC → 空白を除く → 1 文字ずつ」で MeCab 版と 102 件で一致している（`ja_pipeline.ja.md`）。
- **end-to-end の音声翻訳 model**：日英の両方向を、許諾と Rust の道の両方を満たして動かせる物が無い（3.6）。
- **評価の音声の選び方まで Rust に移す**：manifest に行を固定すれば足りる（3.2）。
- **学習と model の変換の道具を Rust に移す**：PyTorch に依存し、配布済みの model で足りる。
- **mobile/ を Dioxus に作り直す**：依頼の範囲の外で、wire 形式の互換で足りる。
- **`hayamimi-relay` を `hayamimi` の process に入れる**：会話字幕の設計の 2 音源を 1 か所で結ぶ形が変わる。1 段の SSE の時間は `L_tr` に入れて測る。
- **断片の訳の時刻を主指標に入れる**：原文が短いほど速く見え、`L` が負にもなる（3.1）。

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
目標値のうち「Python の値以下」と書いた物は R0 の実測に結びつき、それ以外の数（2000 ms、20%、14,000 MiB）はこの文書が置いた仮の値で、R1b の評議で置き直す（10 の H8）。

| ID | 測る物 | command | 期待値 | 陽性対照 | 陰性対照 |
|---|---|---|---|---|---|
| RP-1 | 認識の誤り率（scorecard と同じ組と採点） | `<ev> scorecard --engine hayamimi --sets testdata/eval_real,testdata/eval_real_zhko,testdata/eval_real_yue --out rp01.json` の後に `cargo test --release --test rp01 -- --ignored` | 言語ごとの平均の誤り率が、R0 の 2 回の Python の出力（`<base>/scorecard_run1.md` と `scorecard_run2.md`、RB-3）の言語ごとの平均以下（ja、zh、ko、yue は CER、en は WER）。yue は Python と同じく OpenCC の t2s で正規化してから採点し、Rust の側は `ferrous-opencc`（0.4.0、Apache-2.0、OpenCC の Rust 実装）の t2s を使う（10 の H24）。LID の正解の数も Python 以上。値は `docs/results/scorecard.md`（ja 0.038、en 0.023、zh 0.066、ko 0.081、yue 0.061）と並べて記録する | `<base>` の Python の値を同じ test が読み、scorecard.md と 0.005 以内で一致する | 句読点の段を外した build では ja が悪くなり落ちる |
| RP-2 | ja の golden 8 本 | `cargo test --release --test rp02 -- --ignored` | 各 clip の確定文の FLEURS の参照文に対する CER が、`golden.json` の `cer_finals_vs_reference` 以下（清書の `cer_refine_vs_reference` は RP-19 が写す `test_ja_golden.py` の case が見る）。GPU で出た文字列は `tests/golden/ja/golden_gpu.json` に別に記録する | 8 本の参照文と記録 CER を読めること | ITN を外した build で `ja_085` と `ja_090` が落ちる |
| RP-3 | 主指標 `L`（発話の終わり → 原文が `final` と一致した訳の字幕） | Python と Rust を同じ機械で順に起こし、`<ev> stream --wav lat-ja.wav --repeat 5 --out <run>.jsonl` を各 1 回、`cargo test --release --test rp03 -- --ignored` が 2 本を読む。R0 で決めた `--min-silence` の値を両方に渡す | ja→en：Rust の `L` の p50 と p95 が、Python（`--translate en`、FuguMT）の p50 と p95 以下。`L` の比較集合（3.1。R0 の Python の 2 回では、発話 5、10、18、19 を除く 19 発話）のうち 17 発話以上で、Rust の 5 回の中央値が Python の 5 回の中央値以下（17 は比較集合の発話の数 × 20 / 23 の切り上げで、仮の値。10 の H8）。en→ja（lat-en）：p95 が 2000 ms 以下（仮の値）。`L_asr` と `L_tr` を別の列で出し、`L_tr` が負の発話の数を出す | 同じ test が Python の run から p50 と p95 を出せ、各 run で 有効 + final なし + 終端不正 が枠の 23 × 5 と一致し、有効な標本の全件で `L_asr − L_dec ≥ min_silence` が成り立つ（3.1 の分割式） | 訳の依頼に 600 ms の待ちを入れた偽の llama-server で落ちる。原文が `final` と一致しない訳だけを返す偽の中継では `t_tr` が取れず落ちる |
| RP-4 | 認識の遅れ `L_asr` | RP-3 の同じ 2 本 | `L_asr` の p50 と p95 が Python 以下。`L_asr` の比較集合（3.1）で、Rust の 5 回の中央値が Python 以下の発話が、比較集合の発話の数 × 20 / 23 の切り上げ以上（仮の値。10 の H8）。`L_dec`（各実装の log の値）は記録だけ | 同じ run の全 `final` の `latency_ms` の平均と本数を、WS の `final` event と Python の stdout の log の 2 つから別々に出し、本数が同じで平均の差が 0.5 ms 以内（WS の `latency_ms` は丸めない値で、Python の stdout の log は `latency=` を整数 ms に丸めて出す。1 本の丸めの誤差は 0.5 ms 以内なので、平均の差も 0.5 ms を超えない）。発話の最後の `final` だけの平均とは母集団が違うので比べない | `provider` を `cpu` にした Rust の run の値を同じ式で出し、記録する |
| RP-5 | 速報の字幕と速報の訳 | RP-3 の同じ 2 本 | `t_partial − ` 発話の始まりの sample を送った時刻、の p95 が Python 以下。速報の訳が出た発話では、その時刻が `t_tr` より前 | Python の run から最初の `partial` の時刻が取れる | 速報の訳を切った設定で、速報の訳の数が 0 |
| RP-6 | 推論が GPU で走ったこと（provider の log と nvidia-smi） | `<hy> --wav tests/golden/ja/ja_014.wav --no-realtime --mode single --lang ja --gpu-diagnose` を走らせながら `nvidia-smi --query-compute-apps=process_name,used_memory --format=csv` を 1 秒ごとに読み、`cargo test --release --test rp06 -- --ignored` が log と出力を読む | `hayamimi` の process が compute app に出て `used_memory` が 0 より大きい。3.5 の表で GPU とした段の `gpu.session.created` の全件で実際の provider が `cuda`、CPU とした段は `cpu`。`gpu.session.placed` で CPU の node の数を model ごとに記録し、3.5 の表で GPU とした model は、`gpu_stages.md` の node の表（「CUDA の node」と「CPU の node」の表）の R1 の値以下（句読点の BERT は 77）。GPU とした model が node の表に行を持たなければ落ちる。3.5 の表で CPU とした model と、表に載らない model は記録だけにする | 同じ式で llama-server の process が compute app に出る | `CUDA_VISIBLE_DEVICES` を空にして起こした run では compute app に出ず、`gpu.device.unavailable` が 1 件 |
| RP-7 | 段の間の転送 | R1 の計測（RP-23）の中で `nsys profile --stats=true <hy> --wav ...` を打ち、`cargo test --release --test rp07 -- --ignored` が統計を読む | 1 発話あたりの host → device と device → host の回数、bytes、時間の p50 と p95 が `docs/results/gpu_stages.md` に在る。転送の時間の p95 が認識の時間（`decode_ms`）の p95 の 20% 以下（仮の値） | 同じ式で encoder の kernel の時間も出る | int8 の model を CUDA で読ませた run では転送の回数が増える |
| RP-8 | CPU に残した段の根拠 | `cargo test --release --test rp08` が `docs/results/gpu_stages.md` と 3.5 の表を突き合わせる | 3.5 の表で CPU とした段の全行が、3.5 の根拠 1〜4 のどれかを持つ。根拠 1 は、根拠の列の「根拠 1（variant `<名>`）」が名指しした 1 つの variant だけについて、`gpu_stages.md` の GPU と CPU の行の対で CPU の p50 が GPU の p50 より小さい。同じ段のほかの variant の対は、CPU と GPU のどちらが速くても判定に使わない。variant を混ぜた最小値で比べない。名指しが無い、2 つ以上在る、名指しした variant の対が欠けている、のどれも落ちる。根拠 2 は GPU の `transfer-floor` の行と CPU の行の対で CPU の p50 が小さい。根拠 3 は CPU の p50 が `L` の見積りの p50 の 1% より小さい（仮の値。10 の H8）。根拠 4 は、`gpu_stages.md` の CPU の行が未測定の間は根拠の列に持ち主の slice（R2、R4、R5）の名前と GPU の経路が無い理由（取り込みは一次資料の URL）が在り、測った後は CPU の p50 が GPU の往復の下限（3.5）より小さい | GPU に置いた段の行も、GPU と CPU の p50 と p95 を同じ形で持つ | 根拠の列を空にした表で落ちる。根拠 4 の行の CPU の p50 を GPU の往復の下限と同じ値にした結果で落ちる。CPU の int8 の行だけが GPU の fp32 より速く、同じ fp32 の対では GPU が速い表で落ちる |
| RP-9 | 実行時の Python が 0 本 | Windows で `<hy> --input mix --serve` と `hayamimi-relay live` を 10 分動かし、1 秒ごとに `Get-CimInstance Win32_Process` で `hayamimi` の子孫の process の名前を集める。`dumpbin /dependents <hy>` の DLL の名前。R11 の後は `git ls-files '*.py' \| wc -l` | 子孫に `python` を含む名前が 0。DLL に `python` を含む名前が 0。`*.py` が 0 本 | R0 の Python の run では同じ式で `python.exe` が 1 以上 | 子 process に Python を立てる偽の build で落ちる |
| RP-10 | Windows で動く | Windows 11 で `cargo test --release --test rp10 -- --ignored`（loopback へ英語の wav を鳴らし、`/events` を読む） | `ready` の後に `final` が 1 件以上、`lang` が `en`、`partial` が 1 件以上。マイクは入力の装置が在るときだけ同じ形（会話字幕の CS-23 と同じ前提） | 同じ test を WSL で `--wav` で打つと通る | 出力の装置を止めた run では `error` の event で止まる |
| RP-11 | WSL で動く | WSL で `cargo test --release --test rp11 -- --ignored`（`--wav` と `/ingest`） | 両方の入力で `final` が出て、`gpu.session.created` の provider が 3.5 の表の置き場と同じ（GPU とした段は `cuda`） | 同上 | `/ingest` に 8 kHz の宣言で 16 kHz を送ると、resample の後の CER が落ちて test が落ちる |
| RP-12 | wire 形式の互換 | R0 で Python の server から録った event と応答（`<base>/wire/`）を、`cargo test --release --test rp12` が Rust の出力と比べる | R0 で録れた event の型ごとに key の集合と値の型が同じ（Rust が足す 3.3 の 5 項目と `src_sha256` は差から除く）。8 本の route と `/ingest` の handshake が同じ status と key を返す。mobile の `remote_event_test.dart` の fixture を Rust の出力で置き換えても `dart test` が通る | Python の録りを Python の録りと比べて差 0 | key を 1 つ消した event で落ちる |
| RP-13 | 訳の品質 | `<ev> fleurs-pairs --targets en,zh,ko,es --n 50 --seed 0 --out pairs/` の後に `hayamimi-relay eval --pairs pairs/ --out hyps/`（中継の `eval` は slice 1 に無く、R8f で足す。llama-server が要る）、`<ev> chrf --pairs pairs/ --hyps hyps/` | 確定の訳の corpus chrF が en 47.57、zh 20.19、ko 25.20、es 42.09 を超える（en は会話字幕の CS-3 と同じ）。V が両方の訳を止めた行は空の hyp で数える | 参照訳を hyp に渡すと 100 | 原文を返した hyp では値を割る |
| RP-14 | Rust の chrF が sacrebleu と同じ値を出す | R0 で sacrebleu 2.6.0 が採点した組（`<base>/chrf_fixture.tsv` と値）を `cargo test --release --test rp14` が読む | 差の絶対値が 0.01 以下 | 同じ fixture の文 1 本 | n-gram の上限を 5 にした実装で落ちる |
| RP-15 | 話者の分離 | `<ev> der --set testdata/eval_diar --collar 0.25` の後に `cargo test --release --test rp15 -- --ignored` | AMI 5 会議の平均 DER が `<base>` の Python の 3 回以上の run の平均以下。Python の回の間の幅（平均 DER の最大 − 最小）を同じ結果の file に書き、Rust との差がこの幅より小さければ揺れの中と読む | `<base>` に Python の `--method refine_diarize` の run が 3 回以上在り、同じ test がその平均と幅を出せる（2 回では幅を決めない）。`diarization.md` の値は入力の音声の sha256 が残っていない記録なので比べない | 清書の再分離を切った run では落ちる |
| RP-16 | GPU の無い機械では理由を出して止まる | `CUDA_VISIBLE_DEVICES` を空にして `<hy> --wav tests/golden/ja/ja_014.wav` を起こす `cargo test --release --test rp16 -- --ignored` | 終了 code が 0 以外、`gpu.device.unavailable` が 1 件、`gpu.session.created` が 0 件、stderr に `nvidia-smi` の名前が在る。`LANG=ja_JP.UTF-8` と `LANG=C.UTF-8` で stderr の文が別の言語 | GPU の在る run では `gpu.device.unavailable` が 0 件で `final` が出る | CPU に落として動き続ける build で落ちる |
| RP-17 | 自分で build した library が開け、ORT が 1 つである（R1 の門） | Windows と WSL で `hayamimi-gpu-probe`（`hayamimi-gpu` crate の bin。`native/target/<triple>/release/` に在る）を走らせ、`cargo test --release --test rp17` が子の process の出力を読む（`rp17.rs` の test は `#[ignore]` を持たないので `--ignored` を付けない。付けると 0 本しか走らない）。Linux 限定の test（T-RP-17-031）は WSL から打つ。`hayamimi` crate は R2 で作るので、`<hy> gpu-probe` はそれまで無い | `cuda_devices: 1`、sherpa-onnx の CUDA の session が 1 本作れて実際の provider が `cuda`、`ort` が開いた ORT の `GetVersionString` が `build.json` の ORT の版（1.31.0。退路へ戻したときは 1.28.2）と同じ、process の中の `onnxruntime` の library が 1 つで、その読み込み元が実行 file と同じ directory（probe の `modules` の origin が `exe_directory`）。CUDA の版は、認識器が開いている間の process の読込 module の名前で見る（`build.json` の申告ではない。名前は大文字と小文字を区別せずに数える）。Windows は `cublas64_13` と `cublasLt64_13` が各 1、`cudart64_12` と `cublas64_12` と `cublasLt64_12` が 0。`cufft64_12` は在ってよい（CUDA 13.4 の toolkit 自身が `cufft64_12.dll` を持つので、`*_12` を一律 0 にはしない）。WSL は `libcudart.so.12` が 0、`libcublas.so.13` が 1。ORT の CUDA の provider の library の中の文字列では CUDA の版を判別しない（`cudart64_12.dll` と `cudart64_13.dll` の文字列を両方持つ）。Windows では、`hayamimi-gpu` を依存に持つ全 crate の bins と tests に 3.4 の 2 本の link 引数が在る（`cargo metadata` で依存する crate を数え、各 crate の build script を読む）。通ったときは、その build の `build.json` の sha256 と probe の出力を `native/target/sherpa-gpu/<os>/rp17_passed.json` に書く（RP-24 が読む） | 2 つの OS で同じ出力の形 | 配布の CPU 版の sherpa-onnx に繋いだ build では provider が `cpu` で落ちる。`onnxruntime` の読み込み元が `system` の run で落ちる |
| RP-18 | 句読点の同一性 | `cargo test --release --test rp18` が `mobile/hayamimi_core/test/fixtures/punct_ja_parity.json` を読む | 全 case で `input_ids` が一致。GPU で `expected` の文字列が全件一致 | fixture の 1 件目 | NFKC を外した token 化で落ちる |
| RP-19 | Python の test の case を Rust へ写したこと | `cargo test --release -- --list` の出力と、test-designer が `rust_port_tests.ja.md` に書く対応表を `cargo test --release --test rp19` が突き合わせる | 対応表の全 386 行が、Rust の test の名前か `写さない：<理由>` のどちらかを持つ。名前を持つ 334 行は list に在り、全部通る。`写さない` を許すのは 3 種だけで、3.2 が消す script の test、この設計で無くなる部品（Python の中の翻訳 worker と翻訳器の pool、文ごとの訳の分割。訳は `hayamimi-relay` が 1 行ずつ頼む、3.6）、外が更新する model の sha256 を動作の gate にする case（sha256 は `models/manifest.json` の記録に留める、3.10）である。`docs/spec/ja_pipeline_spec.json` の `config` を `<hy> config dump` で作り直した値と差 0 | 対応表の 1 行目 | 名前を 1 つ変えた list で落ちる |
| RP-20 | 3.5 の表の全段の置き場が決まったこと | `cargo test --release --test rp20` が 3.5 の表の「いまの置き場」の列に「測る」が残っていないことと、各行が `gpu_stages.md` の行に対応することを確かめる | 「測る」が 0 件、表の行と結果の行の対応が全件 | R1 の前の表（`native/hayamimi-eval/tests/fixtures/rp20_table_before_r1.md`、「いまの置き場」の列に「測る」が 8 件）を陽性対照の入力にし、同じ式が 8 件と数える | 結果の file の行を 1 つ消すと落ちる |
| RP-21 | model の取得に Python が要らない | 空の directory で `<hy> models fetch --minimal --dir <tmp>` | 今の `--minimal` と同じ 4 組（ReazonSpeech、whisper-tiny、Silero、句読点）が 3.5 の置き場に合った版（3.10）で揃い、`models/manifest.json` に sha256 が在る | 既に在る directory で打つと取り直さない | 取得先を壊すと前の file を残して error |
| RP-22 | GPU の memory の上限 | RP-3 の run の間、`nvidia-smi --query-gpu=memory.used --format=csv` を 1 秒ごとに読む | 最大が 14,000 MiB 以下（仮の値。16,311 MiB から、他の process の 1,161 MiB と、余白の約 1,150 MiB を除いた値） | 起動前の値が読める | 上限を、その run で測った最大より 1 MiB 低い値に置いた test の設定では落ちる（比べる式が働くことの確かめ） |
| RP-23 | 段ごとの GPU と CPU の計測（R1） | `<ev> stages --provider cuda --wav lat-ja.wav --out stages_cuda.jsonl` と `--provider cpu` を同じ build で 1 回ずつ（1 stream）打ち、同じ 2 本を llama-server に訳の依頼を流し続けた負荷の下でも打ち、`cargo test --release --test rp23 -- --ignored` が `docs/results/gpu_stages.md` を書いて読む | 3.5 の表で GPU の経路が在る段（「いまの置き場」が「CPU（GPU 経路なし）」で始まらない 14 段）について、無負荷と負荷の下の、GPU と CPU の p50 と p95（標本は lat-ja の 23 発話を 5 回流した区切りの数か、文字列の段は同じ数の行）、1 発話あたりの転送の時間と bytes、測った機械と `build.json` の要点が在る。JIT の cache が空の run と温かい run の `decode_ms` が別の行で在る（10 の H27） | 同じ build で同じ入力を 2 回流した p50 の差が 10% 以内 | provider を指定しない run の行は、CPU の行と同じ値になり、GPU の行として受け付けない |
| RP-24 | build の再現（Windows と WSL） | 2 つの OS で `cargo run -p xtask -- sherpa-gpu` を打ち、`cargo test --release --test rp24 -- --ignored` が 2 つの `build.json` を比べる。2 つの OS を比べる `windows_and_wsl_builds_match` は WSL から打つ（WSL の build の directory は symlink で、Windows からは WSL が作った symlink を読めない。os error 1920） | sherpa-onnx の commit、ORT の版、CUDA の主版、cuDNN の主版、`sm_120` 系の判定（3.12）が一致。各 OS で、`sm_120` か `sm_120a` の機械語が 1 件以上、または `sm_120` の PTX があり「PTX の JIT で RP-17 が通った記録」が在る。この記録は、その build の `build.json` の sha256 を持つ `rp17_passed.json`（RP-17）である | 同じ OS で 2 回 build した `build.json` の上の項目が一致 | ORT の版を変えた build では差を名指しして落ちる |
| RP-25 | 訳の揺れ | RP-3 の Rust の run を `cargo test --release --test rp25 -- --ignored` が読む | 発話ごとに、確定の訳が出た後に表示の訳が変わった回数が 0。先の訳から確定の訳への置き換えは 1 回まで。速報の訳で、確定した字形で描いた接頭辞が後で消えた回数が 0。Zoom と Teams と SRT へ出た訳は 1 つの `id` につき 1 件で、確定の訳と同じ | 先の訳と確定の訳が違う発話の数を同じ test が数える | 確定の訳の前に先の訳を送る偽の中継で落ちる |
| RP-26 | 言語の切り替え | `cargo test --release --test rp26` が、`<hy> --help` と、起動の失敗の stderr を、locale を en と ja にして読む | en と ja で文が別の言語で、どちらも `strings.<locale>.toml` の key だけから組まれている。`strings.en.toml` と `strings.ja.toml` の key の集合が同じ | en の出力が英語 | 文言を code に直書きした build で、表に無い文が出て落ちる |
| RP-27 | GPU の経路が無い文字列の段が ORT と CUDA を呼ばない（R2） | `cargo test --release --test rp27` が `rg -c -i '\bort\b\|cuda' native/hayamimi-asr/src/itn.rs native/hayamimi-asr/src/punct/rules.rs` を子の process で打つ | 2 つの file が在り、当たりが 0 件（rc 1）。file が無いときの rc 2 は落ちる | 同じ式を `native/hayamimi-gpu/src/` に掛けると 1 件以上（rc 0） | `itn.rs` に `use ort` の 1 行を足した fixture の木で落ちる |

## 7. test の名前

各 ID の下の case 行（`T-RP-<n>-nnn`）は test-designer が `rust_port_tests.ja.md` に書く。

| ID | 受入 test の名前 | 置き場 | 陰性の case |
|---|---|---|---|
| RP-1 | `rp01_scorecard_error_not_worse_than_python_baseline` | `native/hayamimi-eval/tests/rp01.rs` | 句読点を外した build |
| RP-2 | `rp02_ja_golden_cer_not_worse_than_recorded` | `native/hayamimi-asr/tests/rp02.rs` | ITN を外した build |
| RP-3 | `rp03_speech_end_to_translation_beats_python` | `native/hayamimi-eval/tests/rp03.rs` | 600 ms 遅い偽の llama-server、断片の訳だけを返す偽の中継 |
| RP-4 | `rp04_recognition_latency_beats_python` | `native/hayamimi-eval/tests/rp04.rs` | provider が cpu の run（記録） |
| RP-5 | `rp05_partial_and_partial_translation_arrive_early` | `native/hayamimi-eval/tests/rp05.rs` | 速報の訳を切った設定 |
| RP-6 | `rp06_inference_runs_on_cuda` | `native/hayamimi/tests/rp06.rs` | GPU を隠した run |
| RP-7 | `rp07_transfers_between_stages_are_measured_and_small` | `native/hayamimi-eval/tests/rp07.rs` | int8 の model を CUDA で読ませた run |
| RP-8 | `rp08_every_cpu_stage_has_its_evidence` | `native/hayamimi-eval/tests/rp08.rs` | 根拠の空いた表 |
| RP-9 | `rp09_no_python_at_run_time` | `native/hayamimi/tests/rp09.rs` | Python を子に立てる偽の build |
| RP-10 | `rp10_windows_loopback_reaches_events` | `native/hayamimi/tests/rp10.rs` | 出力の装置の停止 |
| RP-11 | `rp11_wsl_wav_and_ingest_reach_events_on_cuda` | `native/hayamimi/tests/rp11.rs` | 宣言と違う sample rate |
| RP-12 | `rp12_wire_format_matches_python` | `native/hayamimi/tests/rp12.rs` | key を消した event |
| RP-13 | `rp13_translation_chrf_beats_current_models` | `native/hayamimi-eval/tests/rp13.rs` | 原文を返した hyp |
| RP-14 | `rp14_chrf_matches_sacrebleu` | `native/hayamimi-eval/tests/rp14.rs` | n-gram の上限 5 |
| RP-15 | `rp15_der_not_worse_than_python` | `native/hayamimi-eval/tests/rp15.rs` | 再分離を切った run |
| RP-16 | `rp16_no_gpu_stops_with_a_reason` | `native/hayamimi/tests/rp16.rs` | CPU に落として動き続ける build |
| RP-17 | `rp17_self_built_cuda_library_opens_one_onnxruntime` | `native/hayamimi-gpu/tests/rp17.rs` | 配布の CPU 版に繋いだ build |
| RP-18 | `rp18_punctuation_matches_parity_fixture` | `native/hayamimi-asr/tests/rp18.rs` | NFKC を外した token 化 |
| RP-19 | `rp19_python_cases_are_ported` | `native/hayamimi-eval/tests/rp19.rs` | 名前を変えた list |
| RP-20 | `rp20_every_stage_placement_is_measured` | `native/hayamimi-eval/tests/rp20.rs` | 結果の行を消した file |
| RP-21 | `rp21_models_fetch_without_python` | `native/hayamimi/tests/rp21.rs` | 壊した取得先 |
| RP-22 | `rp22_vram_stays_under_budget` | `native/hayamimi-eval/tests/rp22.rs` | 上限なしの LRU |
| RP-23 | `rp23_every_stage_is_timed_on_gpu_and_cpu` | `native/hayamimi-eval/tests/rp23.rs` | provider を指定しない run |
| RP-24 | `rp24_windows_and_wsl_builds_match` | `native/xtask/tests/rp24.rs` | ORT の版を変えた build |
| RP-25 | `rp25_translation_flicker_is_bounded` | `native/hayamimi-eval/tests/rp25.rs` | 先の訳を外へ送る偽の中継 |
| RP-26 | `rp26_messages_follow_the_locale` | `native/hayamimi/tests/rp26.rs` | 文言を直書きした build |
| RP-27 | `rp27_string_stages_do_not_import_ort_or_cuda` | `native/hayamimi-asr/tests/rp27.rs` | `use ort` を足した `itn.rs` |

test は会話字幕の設計 7.1 と同じ規則で書く（`Result` を返して `?`、`unwrap` と `expect` と `unwrap_or*` を書かない、実行時に環境変数を読まない）。
`CUDA_VISIBLE_DEVICES`、`CUDA_CACHE_PATH`、`LANG` は test の code が読む値ではなく、test が子の process を起こすときに渡す値である。

## 8. slice の順

会話字幕の slice 1（`native/hayamimi-relay`、順 1〜13）は済んでいる。
R10 までの各 slice は Python の file を消さないので、slice の commit を revert すれば前の状態に戻る。
Python を消すのは R11 だけで、R11 の commit を revert すれば 78 本が戻る。

| slice | 中身 | 通す受入 | 戻し方 |
|---|---|---|---|
| R0 | 基準を固める。`hayamimi-eval` の `stream`、`record`、`fetch-set`、`fleurs-pairs` を作る。Windows の `.venv` に `pip install sacrebleu==2.6.0 jiwer==4.0.0 simpleder==0.0.5 OpenCC==1.4.2 pyarrow==25.0.1` を打つ（OpenCC が無いと yue の CER が t2s の正規化なしで黙って変わり、pyarrow が無いと FLEURS の対と評価の組が取れない）。今の Python を 2 回以上（DER は 3 回以上）動かし、遅れ、scorecard、golden、chrF の fixture、DER、wire の録りを `testdata/baseline/` に書く。`--min-silence` を 0.20、0.25、0.30、0.35 の 4 つで流して `L_asr` と ja の CER を測り、CER が 0.35 の時を超えない最小の値を、R1 以降の Python と Rust の比較に使う値に決める。要約を `docs/results/baseline_python.md` に書く | RB-1〜RB-8（`rust_port_tests.ja.md`） | `pip uninstall` で 5 つを外す。crate を消す。Python の file は触らない。`docs/results/scorecard.md` は各回の前に `<base>` へ cp で退避し、後に cp で書き戻して `git diff --ignore-cr-at-eol --quiet` で確かめる（RB-3） |
| R1 | 計測の slice。最初の手番は 3.12 の「CUDA 13 版の ORT で sherpa-onnx が build でき RP-17 が通るか」で、通らなければ 3.12 の戻し方に従う。3.12 の環境を確かめ（WSL に cuDNN が無ければ入れる）、xtask で CUDA 版の sherpa-onnx を Windows（msvc）と WSL で build し、`hayamimi-gpu` で開き、`hayamimi-eval stages` で 3.5 の GPU の経路が在る段を GPU と CPU の 1 stream ずつ、無負荷と llama-server の負荷の下で測る。結果で 3.5 の表の置き場を決める。句読点の BERT は、`ort` と `punct_ja_parity.json` の `input_ids` で forward だけを CUDA と CPU に掛ける。認識は、encoder の model だけを `ort` で同じ fp32 の同じ入力に CUDA と CPU で掛ける 1 回と、fp16 の `gpu.session.placed` の CPU の node の数の 1 回を取る。3.5 の表で GPU とした model（言語判定の whisper-tiny の encoder と decoder を含む）の CUDA と CPU の node の数を、`gpu_stages.md` の node の表に全部載せる（RP-6 が読む） | RP-24、RP-17、RP-23、RP-7、RP-8、RP-20 | crate と結果の file を消す。環境は 3.12 の表の戻し方 |
| R1b | 評議。R0 と R1 の実測で、仮の値（RP-3 の 2000 ms、RP-7 の 20%、RP-22 の 14,000 MiB、RP-3 と RP-4 の対の比較の下限）と、`--min-silence` の値を置き直す。`~/.cache/hayamimi-r1-wsl-20261014` に残した cargo の `target` と archive を残すかを決める（3.12 の環境の表の WSL の build の置き場） | 評議の判定を この文書の 6 に写す | 文書の commit を revert |
| R2 | ja の経路（VAD、preroll、ReazonSpeech、分割再試行、ITN、句読点、置換辞書）、`--wav`、最小の server（`/events`）、GPU の無い機械で止まる形。ITN と句読点の後処理の 1 行の CPU の p50 を `hayamimi-eval stages` で測り、`gpu_stages.md` の 2 段の CPU の行に置く。句読点の後処理は BERT を呼ぶ module と分け、`hayamimi-asr::punct::rules` に置く | RP-2、RP-18、RP-6、RP-16、RP-26。RP-8 の根拠 4 の式（ITN と句読点の後処理の CPU の p50 が GPU の往復の下限より小さい）、RP-27 | crate を消す |
| R3 | 言語の振り分け（whisper-tiny と SenseVoice の二重確認、粘り、5 つの tier、Omnilingual、LRU、清書、second opinion、ko の分かち書き） | RP-1、RP-22 | 同上 |
| R4 | 話者（CAM++、pyannote、FastClustering、全体の対応づけ、仮の印）。FastClustering と全体の対応づけの CPU の p50 を `hayamimi-eval stages` で測り、`gpu_stages.md` の「清書の clustering」の CPU の行に置く | RP-15。RP-8 の根拠 4 の式（清書の clustering の CPU の p50 が GPU の往復の下限より小さい） | 同上 |
| R5 | 取り込み（wasapi、cpal、mix、`/ingest`）と server の全 route。取り込みから認識器へ渡すまでと、確定文の event を `/events` へ書き出すまでの CPU の p50 を `hayamimi-eval stages` で測り、`gpu_stages.md` の 2 段の CPU の行に置く | RP-10、RP-11、RP-12。RP-8 の根拠 4 の式（取り込みと配信の CPU の p50 が GPU の往復の下限より小さい） | 同上 |
| R6 | Dioxus の 3 頁（designer の板が先） | RP-12 の頁の行と、overlay の同一性の test | crate を消す |
| R7 | 認識の遅れの詰め。R1 で遅いと分かった段を直す。3.5 の CPU の時間は 1 stream で測った値で、mic と loopback の 2 音源同時の CPU は未測定なので、2 音源を同時に流した CPU の段の p50 と p95 を測る。認識は 2 音源同時で GPU（fp32）と CPU（同じ fp32 と int8）を測り直し、CPU の側が GPU より遅くなれば 3.5 の置き場を評議へ戻す。認識全体の cuda の時間のうち encoder 以外の約 99 ms の内訳を調べる（10 の H31） | RP-4、RP-7 | 設定と commit を revert |
| R8a | 中継が Rust の `hayamimi` の `/events` を購読する（3.3 の 5 項目と `src_sha256`） | 会話字幕の CS-21、CS-22 | 中継の購読先を戻す |
| R8b | llama-server を Windows の native に置く（`%LOCALAPPDATA%\hayamimi\models\llm\`、slot 4） | 会話字幕の CS-20。Windows の CUDA 13.4 の llama-server で T を測り直し、`gpu_stages.md` の T の行を置き換える（RP-23 の T の行の形） | WSL の llama-server に戻す |
| R8c | T の訳を先に出し、E の訳で置き換え、確定の訳だけを外へ出す。会話字幕の T-CX-1-002 を test-designer が書き替える | RP-25、会話字幕の CS-5、CS-25 | 中継の commit を revert |
| R8d | 速報の訳（re-translation、最長共通接頭辞、打ち切り、`cache_prompt`） | RP-5、RP-25 | 設定で速報の訳を切る |
| R8e | 4 つの slot と負荷の下の `L` | RP-3 | slot の数を戻す |
| R8f | `--translate` の言語を llama-server の T で訳す | RP-13 | commit を revert |
| R10 | `models fetch`、`config dump`、`doc-links`、README（`README.md` と `README.ja.md` の CPU だけという売りの行を含む）と文書の書き替え、英語版 | RP-21、RP-14、RP-19 の spec の行 | 文書の commit を revert |
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

## 9. 決めたこと

評議の判定（2026-10-13）で決めた物は、この文書の本文に写した。

| 項目 | 決め | 本文 |
|---|---|---|
| T の訳を先に出す | 採る。V は多くて 2 回、確定の訳だけを外へ出し、揺れを RP-25 で数える | 3.6、会話字幕の 3.4、3.10、3.13 |
| 会話字幕の設計と goals.md の書き替え | この文書と同じ手番で書き替えた。README の 3 行は R10 | 2.4 |
| R0 で Python を動かす | 2 回以上（DER は 3 回以上）。採点と評価の 5 つを `pip install` する | 8 の R0 |
| GPU の無い機械 | 理由を出して止める | 3.11、RP-16 |
| llama-server の置き場 | Windows の native | 3.6、会話字幕の 3.7 |
| Windows の target | `x86_64-pc-windows-msvc` にそろえる | 3.12 |
| Python を消す時 | R11 だけ。3 つの条件の後 | 3.2 |

## 10. 残る穴

- **H1** Parakeet v3（en と欧州 24 言語）と parakeet-ja（second opinion）の fp32 か fp16 の ONNX は、sherpa-onnx の release に見つからない（NOT VERIFIED）。R1 の席が Hugging Face を探し、無ければ int8 を CUDA で動かして CPU に落ちる node の数を `gpu.session.placed` で数え、R1 の表に載せる。そのとき 1.4 の既定を満たせない段として評議へ戻す。
- **H3** `ort` 2.0.0-rc.14 の `load-dynamic` が、自分で build に入れた ORT 1.31.0 を開けるか（NOT VERIFIED。1.31 は `ort` の既定の C API の版で、退路の 1.28.2 では既定を外して古い API の feature を選ぶ）。別の席の調べでは `ort` の配布の binary は CUDA 13.2 以上と cuDNN 9.23 以上を対象にしている（S15）。`load-dynamic` は配布の binary を使わないが、C の API の版の下限が合うかは R1 の RP-17 で確かめる。合わなければ、句読点の model を sherpa-onnx の側で動かす手段を評議へ出す。
- **H5** CUDA の stream を ORT と llama.cpp で共有する手段は無く、同じ GPU を 2 つの process が時間で分け合う。負荷の下の時間は R1 の RP-23 と、RP-3 で測る。
- **H6** llama-server が stream の応答の接続を閉じられたときに生成を止めるか（NOT VERIFIED）。止めないなら、速報の訳の打ち切りは `n_predict` を小さくして待つ形にし、slot を 1 本増やす。R8d の席が確かめる。
- **H7** 発話の終わり `t_end` は manifest に書いた sample の位置で決める。実放送の音声は参照の境界と本当の発話の終わりがずれることがあり、`L` に数十 ms の誤差が入る（ASSUMPTION）。Python と Rust に同じ誤差が入るので、比較には効かない。
- **H8** RP-3 の 2000 ms、RP-7 の 20%、RP-22 の 14,000 MiB、RP-3 と RP-4 の対の比較の下限（比較集合の発話の数 × 20 / 23 の切り上げ。`L` の 19 発話では 17）、3.5 の根拠 3 と RP-8 の 1% は、この文書が置いた仮の値で、実測に基づかない。R1b の評議で R0 と R1 の実測から置き直す。
- **H9** `--min-silence`（0.35 秒）は `L_asr` の最大の部分である。R0 で Python の引数で 4 つの値を測り、Python と Rust の比較に使う値を決める（8 の R0）。Rust の既定は R1b で決める。
- **H10** 確定のとき、最後の速報の解き直しと確定の音声が同じなら、確定の解き直しを省ける見込みがある（ASSUMPTION）。R7 で、同じ音声かを判定する条件と、省いた時の誤り率を測る。
- **H11** sherpa-onnx の中の fbank と探索を分けて測る口が無いときは、3.5 の差の式で出すので、値は推定になる。R1 の結果の file にそのことを書く。fbank か探索が遅い段だと分かっても、sherpa-onnx の中を替えるのは C++ を書く変更になるので、評議へ出す。
- **H12** Nsight Systems が Windows の Rust の実行 file を profile できるかは確かめていない（NOT VERIFIED）。できなければ WSL の同じ build で数える（3.5）。
- **H13** Kiwi（ko の分かち書き）の Rust の binding `kiwi-rs` は Kiwi の C の library を要し、LGPL である。今と同じく任意とし、無いときは素通しにする。Windows の msvc の build から読めるかは R3 で確かめる（NOT VERIFIED）。
- **H14** `docs/results/scorecard.md` の評価の音声は手元に無い（`testdata/eval_real` が無い）。R0 の `fetch-set` が取り直す。取り直した音声が scorecard.md の時と同じかは、R0 の Python の値が scorecard.md と 0.005 以内で一致すること（RB-3）で見る。
- **H16** 英語版 `rust_port.md` は R10 の成果物である。それまでこの文書の日本語だけが在る。
- **H17** CAT-Translate-3.3b の GGUF は配布が無く、会話字幕の試しでは自分で変換した（会話字幕の設計 2.3）。変換は Python の道具（llama.cpp の `convert_hf_to_gguf.py`）を使う。訳の model を決める会話字幕の設計 9 の Q1 が CAT を選んだときに、変換を Python 以外で行う手段が無ければ、その 1 回の変換を規約の例外とするかを評議へ出す。
- **H18** 別の席の調べの「GPU の方が遅い」「GPU の道具が無い」は推測を含む転記で、この文書は根拠に使っていない。3.5 の根拠は R1 の実測と一次資料である。
- **H19** 退路へ戻したときに WSL で使う保守者の `onnxruntime-linux-x64-gpu_cuda13-1.28.2-patched.zip` が、Microsoft の版とどこが違うかは確かめていない（NOT VERIFIED）。1.31.0 は両 OS とも Microsoft の asset なので、この差は退路を使うときだけ効く。RP-24 で 2 つの OS の版の名前をそろえて記録する。
- **H20** hooks の `guard_commands.rs` の llama.cpp の鮮度の検査は、`llama-` で始まる名前を含む command を、WSL の build の木の鮮度で判定する。Windows の llama-server を WSL の shell から起こすと、関係の無い木で refuse される。Windows の llama-server は PowerShell から起こすと決めた（3.6）が、検査の対象を名前でなく path で分ける直しが要るかは、R8b の手番で hook-engineer の席へ出す。
- **H24** `ferrous-opencc` の t2s が、Python の OpenCC 1.4.2 の t2s と同じ文字列を出すかは確かめていない（NOT VERIFIED）。R3 で、R0 の yue の参照文と認識文の全件を両方に通し、差の行の数を数える。差が在れば、yue の CER の差がその行で説明できるかを RP-1 の結果に書く。
- **H22** `state.json` の `constraints` に「Rust への移植は速度が足りないと測れたときだけ」が残っていて、この文書の目標と食い違う。state.json を書くセラが直す。
- **H23** 速報の訳が `final` と同じ原文になったときの `L_tr` の負の値は、速報を早く出す形の正しい効果だが、その発話の数が多いと `L` が小さく見える。RP-3 の出力でその数を見て、多ければ R1b で読み方を決める。
- **H25** Python の `asr_engine.py` の 1715 行目は、早めに判定した言語を 4 秒未満の発話にだけ使い、それより長い発話では確定のたびに言語判定をやり直す。やり直し 1 回あたり 114 ms で、`L_asr` に入る。Rust でこの規則を写すか変えるかは、R1 の言語判定の GPU と CPU の実測（3.5）の後に R3 が決める。
- **H26** `end_sample` の今の規則（20 ms の frame の音量）は、声の後ろに音楽が続く発話（lat-ja の 18 と 19）で声の終わりを決められず、その標本は終端不正に入る。全発話に当てる別の規則（Silero VAD の区切り、手で付けたラベル）で直すかは決めていない（NOT VERIFIED）。発話 18 と 19 は Python の訳が 1 回も取れないので、`L` の比較集合には入らない。直すなら 3.1 の規則のとおり全発話を置き直し、R0 の席が Python を測り直す。
- **H27** 機械語の当たらない kernel を driver が PTX から変換する初回の遅れは測っていない（NOT VERIFIED）。R1 の RP-23 で、JIT の cache が空の run（子の process に `CUDA_CACHE_PATH` で空の directory を渡す。S18）と、同じ cache で続けて流した温かい run の `decode_ms` を別の行に記録する。差の読み方は R1b で決める。
- **H28** Windows で `onnxruntime.dll` の import が System32 の 1.17.1 に結びつく条件は特定していない（NOT VERIFIED）。3.4 の遅延読込みと先読みは probe の 4 回で防げたことだけを確かめている。test の実行 file は cargo が `target/<triple>/<profile>/deps/` に置き、`sherpa-onnx-sys` が DLL を写す `target/<triple>/<profile>/` と directory が違うので、test の process で「実行 file の隣」を先読みしたときに `onnxruntime.dll` が在るかも確かめていない（NOT VERIFIED）。R1 の席が `rustc-link-arg-tests` を足す手番で、test の process の `modules` の origin を数える。
- **H29** 3.5 の「負荷の下」の値は、WSL の llama-server と Windows の ORT が同じ GPU を分けた値で、本番の分け方（Windows の llama-server と Windows の ORT）ではない（NOT VERIFIED）。訳 T の GPU の値は `sm_120` を持たない CUDA 12.0 の build で測ったので、悪い向きに偏りうる（3.12）。R8b が Windows の CUDA 13.4 で T を測り直し、R8e が本番の分け方の負荷の下で `L` を測る（RP-3）。GPU の往復の下限（219 µs）も R1 の計測の値で、測り直していない。
- **H30** 認識の fp16 の GPU が fp32 の 2.7 倍遅い理由は未特定である（NOT VERIFIED）。`ort` の session で CUDA の時に CPU に置かれる node は fp32 が 1599、fp16 が 1611 で（`gpu_stages.md:237-238`）、差の 12 node では説明できない。認識は CPU に置くので置き場には効かないが、GPU で fp16 を選ぶ段が他に出たときに同じ形で遅くなりうる。
- **H31** 認識全体の cuda の時間（同じ fp32 の無負荷の p50 171414 µs）のうち、encoder だけの時間（66262 µs）と fbank（約 6 ms）を除いた約 99 ms が何に使われているか説明できない（NOT VERIFIED）。転送は 1 発話あたり host → device 1524 µs と device → host 935 µs で（`gpu_stages.md:213-214`）、この差を説明しない。node の置き場は `ort` の session の値で、sherpa-onnx の session の置き場は確かめていない。持ち主は R7 で、内訳を調べ、encoder と探索を別の provider に分けずに縮める手段が在るかを評議へ出す。

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
| S14 | llama-server の `/completion` は `prompt` に文字列か token の id を受け、`stop` を受ける | https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md |
| S15 | 別の席の調べの転記：llama.cpp の release に win の CUDA 13 の build、`ort` の配布の binary は CUDA 13.2 以上と cuDNN 9.23 以上、TensorRT の初回は cache なしで 384 秒、end-to-end の model（Whisper translate、SeamlessStreaming、Canary、Parakeet、Riva）が使えない理由 | web-researcher の席の報告（2026-10-13、セラ経由の転送） |
| S16 | sherpa-onnx v1.13.8 の cmake：`onnxruntime-win-x64-gpu.cmake` の 22 行目が「Requires cuda 12.x, cudnn 9.x」、23 行目が Microsoft の `onnxruntime-win-x64-gpu_cuda12-1.28.2.zip` を指す。`onnxruntime-linux-x86_64-gpu.cmake` の 22 行目が「Requires CUDA 12, cudnn 9」、23 行目が保守者の `onnxruntime-linux-x64-gpu_cuda12-1.28.2-patched.zip` を指す。`onnxruntime.cmake` の 156〜178 行目は、`SHERPA_ONNXRUNTIME_INCLUDE_DIR` と `SHERPA_ONNXRUNTIME_LIB_DIR` が在ればその `onnxruntime.dll` と `onnxruntime.lib` を使う。ORT v1.28.2 の Windows の GPU の asset は `onnxruntime-win-x64-gpu_cuda12-1.28.2.zip` と `onnxruntime-win-x64-gpu_cuda13-1.28.2.zip` の 2 つ。保守者の `csukuangfj/onnxruntime-libs` の v1.28.2 に `onnxruntime-linux-x64-gpu_cuda13-1.28.2-patched.zip` が在る | https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/cmake/onnxruntime-win-x64-gpu.cmake 、https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/cmake/onnxruntime-linux-x86_64-gpu.cmake 、https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/cmake/onnxruntime.cmake 、https://api.github.com/repos/microsoft/onnxruntime/releases/tags/v1.28.2 、https://api.github.com/repos/csukuangfj/onnxruntime-libs/releases/tags/v1.28.2 |
| S17 | llama.cpp の release b11583（2026-10-13T09:34Z）の Windows の CUDA の build は `llama-b11583-bin-win-cuda-12.4-x64.zip`、`llama-b11583-bin-win-cuda-13.4-x64.zip`、`cudart-llama-bin-win-cuda-12.4-x64.zip`、`cudart-llama-bin-win-cuda-13.4-x64.zip` ほか。`sm_120` を持つ最初の CUDA が 12.8 であることは NVIDIA の CUDA 12.8 の release notes による（この席は原典を開いていない。転記） | https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=5 |
| S18 | CUDA の JIT の cache：`CUDA_CACHE_PATH` が cache の directory を決め（既定は Linux が `~/.nv/ComputeCache`、Windows が `%APPDATA%\NVIDIA\ComputeCache`）、`CUDA_CACHE_DISABLE=1` で cache を使わない。埋め込みの機械語が当たらない kernel は PTX から変換される | https://docs.nvidia.com/cuda/cuda-programming-guide/05-appendices/environment-variables.html |
| S19 | ORT v1.31.0（2026-10-09 公開）の CUDA 13 版の asset：`onnxruntime-win-x64-gpu_cuda13-1.31.0.zip`（309,484,217 bytes、sha256 `d332ccf8…552f5`）、`onnxruntime-linux-x64-gpu_cuda13-1.31.0.tgz`（244,574,938 bytes、sha256 `2867dec9…81135`） | https://api.github.com/repos/microsoft/onnxruntime/releases/tags/v1.31.0 |
| S20 | sherpa-onnx v1.13.8 の FastClustering は `Eigen/Dense` と `fastcluster-all-in-one.h` だけを include し、`fastclustercpp::hclust_fast` の complete linkage で群を作る。ORT と CUDA を使わない | https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/sherpa-onnx/csrc/fast-clustering.cc |
