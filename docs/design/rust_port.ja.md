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
- **終端不正**：`final` は在るが、`L_asr − L_dec` が有効の線より短い。有効の線は run に渡した `--min-silence` の値（以下 `min_silence`）である。例外は 3 つである。RP-66 と RP-67 は、全標本で線を 0.12 秒（3.17 の早い detector の無音）として数える。RP-32 は、Rust の標本ごとに、その標本を閉じた `final` を区切った規則（`hayamimi` の log の `asr.endpoint.decided` の `rule`。3.13）で線を選び、`rule` が `early` なら `EARLY_MIN_SILENCE_MS`（`native/hayamimi-asr/src/vad.rs` の定数）、それ以外（`silence`、`max_speech`、`flush`）なら `min_silence` とする。Python の標本は早い規則を持たないので、RP-32 でも `min_silence` で数える。早い規則で確定した標本は `L_asr − L_dec` が `min_silence` より短くなりうるので、`min_silence` で数えると速く確定した標本ほど有効から外れるからである。RP-32 が全標本の線を下げずに行ごとに選ぶのは、`silence` の規則で区切った標本まで線を下げると、`t_end` が声の終わりより後ろに置かれた標本（下の段落）を有効に入れ、その短い `L_asr` で Rust を速く見せるからである。RP-66 の test は 0.30 秒と 0.12 秒の両方で、RP-32 の test は全標本を `min_silence` で数えた値と行ごとの線で数えた値の両方で、有効な標本の数を記録する。
- **有効**：`final` が在り、`L_asr − L_dec` が有効の線以上。

終端不正を分けるのは、VAD は声の終わりの後に `min_silence` の無音を数えてから区切るので、`t_end` が本当の声の終わりに在れば `L_asr` は `L_dec` より `min_silence` 以上長くなるからである。
この差が `min_silence` より短い標本は、`t_end` が声の終わりより後ろに置かれている（声の後ろに音楽が続く clip で起きる）。
その標本の `L_asr` は短く出るので、p50 と p95 にも対の比較にも入れない。
`L_asr` の p50 と p95 は有効な標本で取り、`L` と `L_tr` は有効な標本のうち訳が取れた物で取る。
3 つの数と、訳の取れない有効な標本の数は、結果の file に別々に書く。
`--min-silence` の掃引（`rust_port_tests.ja.md` の RB-6）の `L_asr` も、各値の run をその run に渡した `min_silence` で同じ式に掛けて仕分け、有効な標本で取る。
掃引で選んだ値は R1 以降の比較に使い、その比較は有効な標本で数えるので、掃引の表だけ終端不正を含めると、同じ設定の行が比較の時より短く出るからである（0.35 の 1 回目では、全標本の p50 が 1161 ms、有効な標本の p50 が 1185 ms）。

`t_end` を決める規則（manifest の `end_sample` の置き方）は lat-ja の全発話に同じ 1 つを当てる。
規則を変えるときは全発話の `end_sample` を同じ新しい規則で置き直し、Python の固定した記録（py1〜py3）を、新しい `end_sample` で数え直す。Python は流さない。数え直した値には、H42 と H43 の未検証を添える。
一部の発話の `end_sample` だけを結果を見て動かすと、測る物差しを結果に合わせて曲げることになるからである。
今の規則は 20 ms の frame の音量で声の終わりを決める（`baseline_python.md` の RB-2）。
規則の中身は `native/hayamimi-eval/src/manifest.rs` の `speech_end` である。
clip の frame の音量の 10 パーセンタイル（静かな側）から 95 パーセンタイル（大きい側）までの 35 % の高さ（大きい側から 25 dB より下へは下げない）を、5 frame（100 ms）続けて超えた最後の区間の終わりを、声の終わりとする。
`start_sample` は clip の最初の sample で、声の始まりではない（同じ file の `concat` がそう置く）。
`L` は `t_end` からだけ数えるので、始まりのずれ（音量の規則で 0〜2,260 ms、Silero の区切りの始まりで −23〜3,547 ms）は `L` に入らない。

**起点の確かさ**：`end_sample` が声の終わりにどれだけ近いかは、clip ごとに次の 3 つに分かれる。
分け方は 1 つの規則で、全発話に同じに当てる。

- **確か**：規則の高さを 35 % から 25 %、15 %、10 % へ下げても声の終わりの動きが 40 ms 以下（40 ms を含む）で、終わりの後の 300 ms（15 frame）の frame の dB 値の算術平均が規則の高さより 6 dB 以上低い。電力で平均すると組が崩れる（en_03 は 9.0 dB が 2.6 dB になる）ので、dB 値で平均する。
- **切れ**：声の高さの音が clip の最後の frame まで続き、`end_sample` が clip の最後の frame（20 ms）の中に在る。clip の境で音が止まり、その後はつないだ無音である。
- **不確か**：どちらでもない。clip の音量の幅（95 パーセンタイル − 10 パーセンタイル）が狭く、声の尾と背景の音が音量で分かれない。

lat-ja は確か 4（発話 1、6、8、16）、切れ 8（10、11、13、14、19、20、21、22）、不確か 11（2、3、4、5、7、9、12、15、17、18、23）で、lat-en は 15 発話とも確かである。
確かの発話では、`end_sample` は音量で見た声の終わりと 40 ms 以内で一致する。
不確かの発話の声の終わりは `end_sample` より後ろに在りうるが、どこかは音量では決まらない（高さを 25 % にすると 0〜500 ms、15 % にすると 20〜2,020 ms 後ろへ動く）。
切れの発話は clip の終わりで音が止まるので、それより後ろに待つ音は無い。
ただし、切れた音が声か声でない音かは、音量では分からない。
後ろの 300 ms の低さの線を 6 dB から 10 dB に上げると、確かは lat-ja で 3（発話 16 が不確かへ移る）、lat-en で 12 になる。
下の RP-67 と RP-32 への影響の結論は、どちらの線でも同じである（6 の RP-73）。

起点の誤りの向きは 2 つである。
不確かの発話では `t_end` が声の終わりより前に置かれうるので、`L` が大きく出る。受入には厳しい向きである。
`t_end` が声の終わりより後ろに置かれた標本は、終端不正の式で外れる。ただし外れるのは、その標本の `final` が `t_end` から有効の線の内に来た時だけである。
そのため、RP-67 の合否が起点の確かな発話だけで数えても変わらないことを、RP-73 で確かめる。

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
2. RP-1、RP-13、RP-14、RP-15 が `<base>` の値と照らして通っている。RP-13 は 6 の RP-13 の行の決め方（V なしの chrF を記録と同じ組で測る）で、この入口の条件は満たした（V なしは 4 言語とも記録を超えた。2026-10-15 05:1x、1 回。再現と版による揺れは NOT VERIFIED）。V ありの値は止め率つきの報告に留める。RP-19 は 187 本が未実装で、8 の「RP-19 の未実装 187 行の仕分けと slice」の slice（Refiner の R12w0〜R12w4 を含む）が全部入るまで R11 は始めない。RP-1 の ko の R11 の入口の上限（RP-36）は、配る形の build の記録値 86939（百万分率）で、Rust の平均を丸めずに整数のまま比べ、超えたら評議へ出す（小数 3 桁へ丸めるのは Python の表と比べる RP-1 の本体だけ）。RP-1 の閾値の 0.081 は動かさず、ko の行は赤のまま 10 の H35 に残す。陽性対照は、Python の wheel の ORT を読ませた Rust の run の 66 clip が Python の log と全部一致し、ko が 0.081 になること（R3c の実測）である。ORT か sherpa-onnx の版を上げるたびに ko を測り直し、86939 と比べる。
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

**1 つの認識器の provider は 1 つ**：v1.13.8 の offline の transducer は、encoder、decoder、joiner の 3 つの session に同じ 1 つの session の設定（`GetSessionOptions(config)`）を渡す（`sherpa-onnx/csrc/offline-transducer-model.cc`、https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/sherpa-onnx/csrc/offline-transducer-model.cc 、取得 2026-10-20）。
C の API の `SherpaOnnxOfflineModelConfig` も `provider` を 1 つだけ持つ（S11）。
探索の loop は host で回るが、loop が frame ごとに呼ぶ decoder と joiner は認識器の `provider` で走る（`modified_beam_search` は 1 frame に decoder と joiner を 1 回ずつ呼ぶ。https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/sherpa-onnx/csrc/offline-transducer-modified-beam-search-decoder.cc 、取得 2026-10-20）。
したがって `provider` を `cuda` にすると、小さい decoder と joiner の 1 回ごとに host と device の往復が入る。
同じ library の online の側は、OpenVINO のときだけ decoder と joiner を CPU の session に置き、encoder だけを加速器に置く（`sherpa-onnx/csrc/session.cc` の `GetSessionOptions(const OnlineModelConfig &, const std::string &model_type)`。理由は「decoder と joiner は小さく遅れに効く graph で、計算のほぼ全部は encoder に在る」と書かれている。master で読んだ。https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/sherpa-onnx/csrc/session.cc 、取得 2026-10-20）。
offline の transducer には同じ分け方が無い。
encoder だけを GPU に置く形は、3.5 の「認識の encoder だけを GPU に置く案」に置く。

**CUDA graph と IoBinding の効く範囲**：ORT の CUDA graph は、全 node が CUDA の実行 provider に置かれ、入力と出力の形と address が呼び出しの間で変わらない model にだけ使える（address は IoBinding で固定する。`If`、`Loop`、`Scan` を持つ model と、複数 thread からの呼び出しは使えない。https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html 、取得 2026-10-20）。
認識の encoder は CUDA の session でも 1599 個の node が CPU に置かれ（`gpu_stages.md` の「node の置き場」の表）、発話ごとに長さが変わるので、CUDA graph の条件を満たさない。
IoBinding について同じ文書が書く効き目は、session の入力と出力の copy を減らすことである。
CPU に置かれた node の入口と出口で session の中に起きる copy は、IoBinding の対象に入らない。
TF32 の既定は、ORT の文書では有効（`use_tf32` の既定が 1）、`ort` の `ep::cuda::CUDA::with_tf32` の文書（https://docs.rs/ort/2.0.0-rc.14/ort/ep/cuda/struct.CUDA.html 、取得 2026-10-20）では無効と書かれていて、食い違う。
sherpa-onnx の CUDA の session と `gpu_stages.md` の `ort-encoder` の run がどちらで走ったかは NOT VERIFIED で、効き目は RP-75 が text の一致で測る。

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
| 認識の encoder（全 model） | CPU（int8） | 根拠 1（variant `fp32`）。sherpa-onnx の認識全体（encoder と探索）の同じ fp32 の対で、無負荷の p50 が GPU 171414 µs に対し CPU 151299 µs、負荷の下は GPU 375176 µs に対し CPU 175025 µs（「認識の encoder」の行、:67-94、:260）。CPU の中では int8 が 128615 µs で fp32 より速いので int8 を読む（int8 は CUDA に op が無く、置き場の比較には入れない。3.4）。encoder だけを `ort` で掛けると GPU 66262 µs に対し CPU 121091 µs で GPU が速いが、provider は認識器の config に 1 つで、C の API のままでは encoder と探索を別の provider に分けられない（S11 の `SherpaOnnxOfflineModelConfig`、3.4 の「1 つの認識器の provider は 1 つ」）。分ける案は表の下の「認識の encoder だけを GPU に置く案」で、評議の席の判定と RP-74 の結果が揃うまで、置き場はこの行のまま変えない。この置き場は測った ReazonSpeech の transducer に限る。他の認識器は R3 が同じ式で測る。CPU が速い向きはどの測りでも同じだが、差は CPU の時間に対して、上の無負荷の値で 13%（171414 対 151299 µs）、別の測り直しの認識全体の p50 で、負荷の低い run が 8%（cuda 174.3 対 cpu 162.0 ms）、load 12 の run が 2%（cuda 192.1 対 cpu 188.4 ms）と小さく、CPU の負荷が重なると CPU の側が悪化する。R7 の 2 音源同時で測り直す |
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

認識の置き場は CPU（int8）のまま変えない。R7 の測り直し（`docs/results/r7/remeasure_20261015/`、peers 1、115 発話、別の process で GPU fp32 と CPU int8 を交互に 3 回ずつ）で、認識全体の p50 の 3 回の中央値が CPU int8 173544 µs、GPU fp32 229705 µs、CPU fp32 200120 µs で、3 回の対のどれでも CPU int8 が GPU fp32 より速かった。
1 回目の GPU の run の前だけ 1 分の load が 2 未満（1.98）で、2 回目以降の run の前は 3.8〜11.4 だった（CPU の run が約 10 core を使う）。load は CPU の run を GPU の run より遅らせる向きに効くので、load が高い下でも CPU int8 が速いという結論は置き場の判断に使う。値そのものは低い load の下の値ではない（NOT VERIFIED）。

段と段の間の転送は、R1 で発話ごとに測る。
sherpa-onnx の中の転送は、CUDA の profiler（Nsight Systems の `nsys profile --stats=true` の `cuda_gpu_mem_time_sum` と `cuda_gpu_mem_size_sum`）で、1 発話あたりの host → device と device → host の回数、bytes、時間を数える。
profiler が Windows で使えないときは WSL の同じ build で数え、そのことを結果の file に書く。

#### 認識の encoder だけを GPU に置く案

認識の 1 回の時間を段に割ると、encoder は GPU が速く、探索は CPU が速い。
上の表の置き場は、両方を合わせた認識全体で GPU と CPU を比べて決めたので、どちらの側も遅い段を 1 つ抱えている。
encoder だけを CUDA に置き、fbank と探索を host の CPU に置けば、両方の速い側を取れる見込みがある。
この形を **split の形** と呼ぶ。
見積りでは、認識の 1 回は無負荷で 128.6 ms から 96.3 ms に縮む。
この −32.3 ms は、`L` の見積りの p50 1349 ms（`gpu_stages.md:304`）の 2.4% にとどまり、3.17 の目標（`L` の p95 を 1000 ms 以下、RP-67）には届かない。
split の形は、CPU を取り合う 2 音源同時で効く部品として採る。
2 音源同時の見積りは 166 ms 以上縮むが、WSL で別の時間帯に取った A と B の run を継いだ値で、Windows の 2 音源同時は測っていない。
GPU の負荷の下では、縮みの幅 2.5〜27.8 ms が RP-74 の線（15 ms）をまたぐので、この見積りでは A との優劣を決めない。
採るのは D で、RP-74 から先へ進める。
置き場は、RP-78 が通るまで上の表の行のまま CPU（int8）に置く。

**見積りの式**：記号を次のように置く。

- **`t_rec`**：認識の 1 回の時間（`create_stream` から `get_result` まで）。
- **`F`**：fbank の時間。**`E`**：encoder の 1 回の時間。**`S`**：探索の時間（`modified_beam_search`、beam 4。1 frame に decoder と joiner を 1 回ずつ呼ぶ）。
- **`X`**：fbank の出力を device へ送り、encoder の出力を host へ戻す 2 回の copy の時間。
- **`N`**：encoder の出力の frame の数（lat-ja の p50 は 146。`r7_recognition.md` の「探索の内訳」）。**`r`**：host と device の 1 往復の下限（`gpu_stages.md` の `transfer-floor`）。

```
A（今）        t_rec = F + E(cpu, int8) + S(cpu, int8)
B と D         t_rec = F + E(cuda, fp32) + X + S(cpu, int8)
差             t_rec(B) − t_rec(A) = E(cuda, fp32) + X − E(cpu, int8)
E(cpu, int8)   = t_rec(A) − F − S(cpu, int8)           int8 の encoder だけの時間は測っていないので引き算で出す
S(cpu, fp32)   = t_rec(cpu, fp32) − E(cpu, fp32) − F    同じ fp32 の対の引き算
S(cpu, int8)   = 0.97 × S(cpu, fp32)                    R7 の search_replay の int8 と fp32 の比（58.4 / 60.1 ms）
C の下限       t_rec ≥ F + E(cuda, fp32) + N × r         探索を GPU に残すと、frame ごとに joiner の出力を host へ 1 往復させる
```

B と D は、decoder と joiner に今と同じ int8 の file を CPU で使う。
したがって `F` と `S` は A と打ち消し合い、差は encoder の 2 つの時間と `X` だけで決まる。
`X` は 0.5 ms と置く（ASSUMPTION。fbank の出力は 1 発話で約 187 KB（146 × 4 frame × 80 bin × 4 bytes）で、`gpu_stages.md` の「転送」の fp32 の host → device は 4367000 bytes を 1524 µs で運んでいる。encoder の出力の幅は数えていない）。

**見積り**（ms、p50）：

| 形 | 無負荷 | GPU の負荷の下 | 2 音源同時 |
|---|---|---|---|
| A 今のまま（sherpa-onnx、全部 CPU の int8） | 128.6 | 151.6 | 396.6 |
| B encoder を `ort` の CUDA に置き、fbank と探索を Rust で host に置く | 96.3 | 123.8〜149.1 | 181.1〜230.3 |
| C sherpa-onnx を CUDA のまま使い、IoBinding か CUDA graph を足す | 105.8 以上（今の CUDA の形は 171.4） | 177.2 以上（今の形は 375.2） | 下限は出せない（今の形は 360.3） |
| D sherpa-onnx に patch を当て、encoder だけを CUDA に置く | 96.3 | 123.8〜149.1 | 181.1〜230.3 |

GPU の負荷の下の B と D の幅は RP-74 の線をまたぐので、この列では A との優劣を決めない。
2 音源同時の列は、WSL の別の時間帯の run を継いだ値である（下の「2 音源同時」）。

値の出所と計算は次のとおりである。
`S` はどの列も引き算か、別の run の `search_replay` から出した値である。
`S` を同じ窓で直に測った独立の値は RP-74 の `search_replay` だけで、Windows の `search_replay` の行は `gpu_stages.md` に 0 行である。

- **無負荷**（Windows、`gpu_stages.md`）：`t_rec(A)` は CPU の int8 の 2 run の p50（:83、:85）の平均 128.6、`F` は CPU の fbank 7.5（:265）、`E(cuda, fp32)` は 66.3、`E(cpu, fp32)` は 121.1、`t_rec(cpu, fp32)` は 151.3（:264）である。`S(cpu, fp32)` は 151.3 − 121.1 − 7.5 = 22.7、`S(cpu, int8)` は 22.0、`E(cpu, int8)` は 128.6 − 7.5 − 22.0 = 99.1 になる。B は 7.5 + 66.3 + 0.5 + 22.0 = 96.3 で、差は −32.3（−25%）である。C の下限は 7.5 + 66.3 + 146 × 0.219 = 105.8 である（`r` は「認識の文字列 → 訳の入力」の無負荷の下限の最小 219 µs、:196-198）。
- **GPU の負荷の下**（Windows、llama-server へ訳の依頼を流し続ける 2 本の client）：`t_rec(A)` は 151.6（:90、:92）、`F` は 8.4（:109-112）、`E(cuda, fp32)` は 92.9、`E(cpu, fp32)` は 117.8、`t_rec(cpu, fp32)` は 175.0（:264）である。同じ引き算の `S(cpu, int8)` は 0.97 × (175.0 − 117.8 − 8.4) = 47.3 で、無負荷の 22.0 の 2 倍を超える。GPU の負荷が CPU の探索を 2 倍にする機構は見つかっておらず、別々の run の引き算の揺れの疑いがあるので、B と D は `S` に 22.0 と 47.3 を入れた幅で書く。差は −27.8〜−2.5 である。C の下限は、`r` を負荷の下の下限 0.52 ms（:199-200 の平均）にした 8.4 + 92.9 + 146 × 0.52 = 177.2 で、A より遅い。
- **2 音源同時**（WSL、R7 の peers 1。`r7_recognition.md` と `docs/results/r7/` の run file）：`t_rec(A)` は 396.6（`r7_recognition.md:39`）、`F` は 13.6、`E(cuda, fp32)` は 103.5（`cuda_p1_encoder_only.jsonl` の `encoder_only` の p50）、`S(cpu, int8)` は search_replay の joiner 17.6 と decoder 45.9 の和の 63.5（`cpu_peers1.jsonl`）である。B の下の端は 13.6 + 103.5 + 0.5 + 63.5 = 181.1 になる。R7 の CPU の run では、sherpa-onnx の decode の時間が encoder と fbank と search_replay の和より長い（10 の H31）。その残りの 49.2 ms（fp32 の decode 464.0 − encoder 318.8 − fbank 13.6 − replay 82.4）が探索の中に在るなら、B と D もそれを持つので、上の端はそれを足した 230.3 とした。同じ窓の peers 0 では、A の 316.4 に対し B は 144.9〜257.4 である。A の値は CPU の run、B の `E(cuda, fp32)` は CUDA の run から取り、R7 の 2 つの run は同じ時間帯に続けて取っていない（`r7/conditions.md`）。この run の 1 分の load は 10〜32 で、Windows の 2 音源同時は測っていない（NOT VERIFIED）。
- **CPU の負荷**（原因分析の席の申告の転記で、設計の席は打ち直していない）：同じ音声の A の decode の中央値が、run の前の load1 が 0.69〜1.36 の run から 1.77〜1.89 の run で 13.5 ms 延びた。B と D で CPU に残るのは `F + S` の 29.5 ms（無負荷の `t_rec(A)` の 23%）である。CPU の負荷による延びが CPU の時間に比例するなら、同じ負荷で延びるのは約 3.1 ms である（ASSUMPTION）。

**今の置き場を書いている物**（2026-10-20、`target/` を除く。pattern ごとの数と陽性対照）：

| pattern と範囲 | 数 | 当たった所 | 案が通ったときの扱い |
|---|---|---|---|
| `rg -c 'epoch-35-avg-1\.int8' native` | 6 file、17 件 | `hayamimi-asr/src/recognizer.rs`（6）、`hayamimi-asr/src/pipeline.rs`（3、test の session の名前）、`hayamimi/src/models_fetch/items.rs`（3）、`hayamimi/tests/rp11.rs`（1）、`hayamimi-eval/tests/rp06.rs`（3）、`hayamimi-eval/src/gpu_nodes.rs`（1） | Rz の encoder を fp32 に替え、`provider` を `cuda` にし、落ちた時は今の int8 の形で開く。`models fetch` は fp32 の encoder を足して取る。int8 の encoder が `cpu` で作られることを期待する test は、fp32 の encoder が `cuda` で作られることに直す |
| `rg -c 'epoch-35-avg-1\.onnx' native`（fp32 の encoder の名前。陽性対照を兼ねる） | 3 file、9 件 | `hayamimi-gpu/tests/rp17.rs`、`hayamimi-eval/src/stage_report.rs`、`hayamimi-eval/tests/rp06.rs` | 変えない（測りと RP-17 の対象） |
| `rg -c 'CPU int8\|CPU（int8）\|認識器を CPU\|認識器は CPU\|認識（ReazonSpeech、CPU' docs/design/` | 4 file、14 件 | この文書の 10 件（この節を足す前に数えた。3.5 の表の行と表の下の 2 行、3.11、3.15、3.16、3.17、8 の R7 の行と Router の段落、10 の H31）。5 の「構造」の末尾の「認識（ReazonSpeech の transducer、int8）… CPU に置き」は、この pattern に当たらない同じ意味の文で、同じく書き替える。`rust_port_tests.ja.md` の 2 件、`status.ja.md` と `status.md` の各 1 件 | 評議の判定と RP-74 から RP-78 の結果を受けた設計の席が、同じ手番で全部を仕分けて書き替える |
| `rg -i 'patch' native/xtask/src` | 0 件の patch の段（当たりは cuDNN の版を読む `CUDNN_PATCHLEVEL` だけ） | `xtask/src/record.rs` | D のとき、patch を当てる段と、`build.json` に patch の sha256 を記録する行を足す（記録で、build の可否の条件にしない） |

コードの pattern の陽性対照は、`recognizer.rs` の `modified_beam_search` が 1 件在ることである。

**方針**：

- **A 今のまま**：手を入れない。ただし、CPU の負荷が重なると認識が延び（上の 13.5 ms）、2 音源同時の見積りで 4 つの形のうち最も遅い。採らない理由は、無負荷と 2 音源同時の両方で B と D に 32 ms 以上負ける見積りだからである。RP-74 が落ちたときは A に残る。
- **B encoder を `ort` の CUDA に置き、fbank と探索を Rust で host に置く**：sherpa-onnx の認識器を Rz の tier に使わず、同じ build の ORT を `ort` で開いて session を 3 つ作る（encoder は CUDA、decoder と joiner は CPU）。fbank と `modified_beam_search` は Rust で書く。session の作成の失敗は `ort` の `Err` で返るので、CPU へ落ちたことの検出は D より確かである。ただし、fbank と探索の 2 つを sherpa-onnx と同じ値が出るように作り直し、その一致を示す道具（下の「B を採るときの front end と探索」）が要る。自分の案にしない理由は、D と同じ時間のために、作り直す部品が 2 つ増えるからである。
- **C sherpa-onnx を CUDA のまま使い、IoBinding か CUDA graph で往復を減らす**：ただし、C の API に IoBinding の口が無く、探索の C++ を書き替える patch が D より大きい。CUDA graph は encoder に使えず（3.4）、decoder と joiner に使えても、探索の top-k を host で取る限り frame ごとの往復は残る（式の下限）。採らない理由は、下限でも B と D より遅く、GPU の負荷の下では A より遅いからである。探索の loop まで GPU の上で回す道（loop の順を入れ替える label-looping と、CUDA graph の条件 node。https://arxiv.org/abs/2406.03791 、https://arxiv.org/abs/2406.06220 、取得 2026-10-20）は、ORT の CUDA graph が control flow の op を持てないので（3.4）、この build の上には無い。
- **D sherpa-onnx に patch を当て、encoder だけを CUDA に置く**：xtask が build する sherpa-onnx v1.13.8 の source に 1 本の patch を当てる。patch は `offline-transducer-model.cc` で、`provider` が `cuda` のときだけ decoder と joiner の session に CPU の session の設定（同じ thread の数）を渡し、encoder の session は今のまま `cuda` の設定を受ける。online の側が OpenVINO のときに持つ分け方（3.4）と同じ形である。Rz の tier の config は、`provider` を `cuda`、encoder を fp32 の `encoder-epoch-35-avg-1.onnx`、decoder と joiner を今と同じ int8 の file にする。fbank と探索は sherpa-onnx の code のまま走るので、A と同じ code が同じ値を出す。ただし、他の project の C++ に patch を持ち、sherpa-onnx の版を上げるたびに当て直す。patch は unified diff の 1 file（`native/xtask/patches/sherpa-onnx-v1.13.8-transducer-split.patch`。実装の席が作る）として repo に置き、xtask は source を取った直後にそれを当て、当たらなければ build を落とす。`build.json` には patch の sha256 を記録だけし、build の可否の条件にしない。patch が効いたかは記録ではなく session ごとの観測で確かめる（下の fallback の 4、RP-77）。また sherpa-onnx は CUDA の provider が無いと黙って CPU に落ちる（`session.cc` の `Fallback to cpu!`）ので、その検出を外から組む（下の fallback の 2）。

**採るのは D である**。
B と同じ時間を、fbank と探索を作り直さずに取れるからである。
言語の順（Rust が先）に対して C++ の patch を選ぶ理由は、変える物が既に build している library の 1 file の数行で、新しい C++ の部品を足さないからである。
D の実装（patch、xtask の段、Rz の config）は RP-74 が通ってから始める。
B は、D の patch が当たらなくなり本家にも入らない時の代わりとして残す。
形の選び方は、`hayamimi`、`hayamimi-eval scorecard`、`hayamimi-eval stages` に `--asr-encoder <cuda|cpu>` を足す（`cuda` が split の形、`cpu` が A の形）。
RP-76 と RP-78 は同じ build の 2 つの形を比べるので、この引数が要る。
CLI の引数のままなら、使い方の文言が RP-26 の対象に入る。
`hayamimi` の config の key にもするなら、`config dump` の既定の値（RP-19）と使い方の文言（RP-26）を、key を足す同じ手番で数え直す。
既定の値は RP-78 の結果で決める。

**B を採るときの front end と探索**：

- **fbank を bit まで揃える**：sherpa-onnx v1.13.8 が固定する kaldi-native-fbank の版の計算（窓、DC の除去、pre-emphasis、実数の FFT、mel の三角の filter、log と energy の下限）を、同じ演算の順と f32 のまま Rust に写す。option の値（dither、snip_edges、周波数の下限と上限、窓の種類、音声の scale）は、sherpa-onnx の offline の zipformer の feature の設定の source から読む（実装の席が、固定した版の source の file と行を brief に貼る。この文書は値を書かない）。浮動小数の和は順で値が変わるので、積和の融合（FMA）と、和の順を変える SIMD の並べ替えを入れない。C の API は fbank の値を外へ出さないので、参照の値は、sherpa-onnx の build に入っている kaldi-native-fbank の library を link した試験用の dump の道具（C++、配る物に入れない）で、23 発話の fbank を f32 の little endian で書き出して作る。Rust の fbank の出力と参照を `f32::to_bits` で比べ、全 frame の全 bin が一致するまで探索の試験へ進まない。fbank がずれると探索の前で text が変わり、探索の一致の試験が何を測ったか分からなくなるからである。
- **探索を同じにする**：`modified_beam_search` の 1 frame は、全ての仮説を 1 回の decoder の呼び出しにまとめ、encoder の frame を仮説の数だけ並べて joiner を 1 回呼び、行ごとの LogSoftmax に仮説の score を足し、仮説 × 語彙の中から上位 4 つを取る（取った token が blank か unk なら token の列を伸ばさない）。同じ token の列になった仮説のまとめ方（`Hypotheses::Add`）と、最後の仮説の選び方（`GetMostProbable(true)`）は、上の file の外に在り、この文書では読んでいない（NOT VERIFIED）。実装の席が固定した版の source を読んで写す。hotword は渡していない（`recognizer.rs` の `HOTWORDS_SCORE` の注）ので、context graph は作らない。
- **一致の受入**：B の encoder を CPU の `ort` で走らせた run と、同じ file を CPU で読んだ sherpa-onnx の run とで、23 発話の token の列と時刻が全部一致することを RP-75 の前段とする。そのうえで encoder を CUDA にした run を RP-75 の本体で比べる。

**GPU が無いときの経路**（B と D で同じ。3.11 の 2 段に入れる）：

1. CUDA の device が 0 か driver が読めなければ、split の形を作らず、A の形（全部 CPU の int8）で認識器を開く。理由は `no_device` か `driver_unreadable` である。
2. device が 1 以上なら split の形を作る。作るのに失敗したら（B は `ort` の `Err`、D は認識器の作成が `None`）、A の形で開き直す。理由は `session_failed` である。D は sherpa-onnx が黙って CPU に落ちうるので、split の認識器を CUDA の session の中で最初に作り、作った直後に言語判定と同じ検出（`hayamimi-asr/src/lid.rs` の `cuda_did_not_run`、CUDA の provider の module が process に読まれたか）を打つ。CUDA が走っていなければ A の形で開き直し、理由は `provider_mismatch` とする。最初に作るのは、先に別の CUDA の session が在ると、module が読まれていても認識器の encoder が CUDA で走った証拠にならないからである。
3. 落ちたら、`gpu.fallback.cpu`（`stage` は `recognizer`、`error_code` は `HAYAMIMI_GPU_FALLBACK`）を 1 回、`/events` に `{"type":"fallback","stage":"recognizer","reason":...,"to":"cpu"}` を 1 回、利用者の locale の文を stderr に 1 回出し、字幕を出し続ける（3.15 の形）。`stage` と `reason` の値は 3.15 の 2 つの一覧に既に在り、一覧は変わらない。

4. D で patch が効いていることは、上の 2 の検出では見えない。`cuda_did_not_run` は CUDA の provider の module が process に読まれたかだけを見て、`lid.rs` の `reports` は要求した provider をそのまま書くので、patch の外れた library で encoder、decoder、joiner の 3 つが CUDA に置かれた形（認識の 1 回が 171 ms の形）でも、この 2 つは CUDA で走ったと答える。C の API は session ごとの provider を返さないので、実行時には確かめず、受入で session ごとの copy の回数を数えて確かめる。decoder と joiner が CUDA に居れば、frame ごとに host と device の往復が足され、1 発話の host → device の回数が encoder だけの時より frame の数に比例して増える（今の CUDA の形は p50 で 450 回、`gpu_stages.md` の「転送」）。RP-77 の後段がこれを数える。

落ち先を fp32 の encoder の CPU ではなく A の形にするのは、CPU の上では A が速く（無負荷の 128.6 ms 対 151.3 ms）、誤り率を RP-1 で測ってある形だからである。

**確かめる probe**：RP-74 が置き場を変える前の門で、build した物を新しく作らずに打てる（`hayamimi-eval stages` の `recognize`、`encoder_only`、`search_replay` は在る）。
B と D の実装は RP-74 が通ってから始め、RP-75 から RP-78 はその実装を受ける。
RP-74 の打ち方と壊れ方は次のとおりである。

- **何を、どの音声で、何回**：lat-ja（23 発話）を `--repeat 5` で流す（1 run 115 標本）。無負荷、GPU の負荷の下（`--load-url <llama> --load-clients 2`）、2 音源同時（`--peers 1`）の 3 条件で、CPU の run（A の `recognize` と、`encoder_only` と `search_replay` の CPU の値）と CUDA の run（`encoder_only`）を 1 組とし、条件ごとに 3 組を別の process で交互に打つ（組の番号が奇数なら CPU の run が先）。`<llama>` は訳の llama-server の URL（3.6）である。
- **B の標本の作り方**：B の見積りは、同じ組の CPU の run と CUDA の run の同じ `(utt, repeat)` の値を、fbank + `encoder_only`（cuda）+ 0.5 ms + `search_replay` の joiner と decoder（int8、cpu）として標本ごとに足し、足した 115 個から p50 と p95 を取る。p95 どうしを足さないのは、各段の遅い標本が同じ発話に揃うとは限らず、和の p95 より大きく出るからである。
- **線**：A 対 A の対照と組にする。条件ごとに、3 組の A の p50 の幅（最大 − 最小）を A の揺れとする。通る条件は、3 組の B の p50 の中央値が A の p50 の中央値より「15 ms と A の揺れの大きい方」以上小さく、3 組の B の p95 の中央値が「A の p95 の中央値 + 3 組の A の p95 の幅」以下であることである。15 ms は、CPU の負荷の差だけで A の decode が延びた幅（原因分析の席の申告で 13.5 ms、評議の席の数え直しで旧と新の平均の差 15.4 ms）に当たる値で、A の 3 run の幅（評議の席の数え直しで 24.6 ms）がそれより大きい窓では、A の揺れの方が線になる。
- **落ちた時**：無負荷か 2 音源同時で落ちたら、D を進めず A に残す。落ちたのが GPU の負荷の下の条件だけなら、A へ自動で戻さず、3 条件の値を付けて評議へ戻す。
- **peers の偏り**：`--peers 1` の 2 つ目の音源は、`stages` を打った provider の認識器を丸ごと回す（`native/hayamimi-eval/src/stage_peer.rs` の `source_precision`。CUDA の run では fp32 の認識全体が CUDA で、CPU の run では int8 が CPU で回る）。CUDA の run の peer は split の形ではなく探索まで CUDA に置いた形なので、B の `encoder_only` は本当の 2 音源同時より重い GPU の取り合いの下で測られ、CPU の取り合いは受けない。偏りの向きは GPU と CPU で逆で、差の大きさは測っていない（NOT VERIFIED）。RP-78 の後段の split の認識器の run が、peer も split の形にして受ける。
- **初めての実行**：JIT の cache が空の process は最初の decode が遅い（`gpu_stages.md` の「JIT の cache と decode_ms」で 529 ms 対 358〜381 ms）。各 run は `--cache warm` の process で打ち、`decode_first` は合否に入れない。
- **起動の形**：A の run も B の run も、peer を含めて全部同じ高い優先度で起こす。WSL は `sudo -n nice -n -5 <command>`、Windows は `cmd.exe /c start "" /high /wait <command>` で起こし、run の間に `Get-Process hayamimi-eval | Select-Object Id,PriorityClass` で peer を含む全 process が `High` であることを `docs/results/rp74/priority.txt` に書く。`High` でない process が在った run は捨てる。
- **取り合う資源**：GPU（E2E の席と llama-server）、CPU（他の席の cargo の build）、llama-server の slot。各 run の直前と直後に、WSL の `pgrep -x rustc` と `pgrep -x cargo`、Windows の `Get-Process -Name rustc,cargo -ErrorAction SilentlyContinue` がどれも 0 件で、`nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv` に llama-server と run 自身の他の compute app が居ないことを数え、`docs/results/rp74/load.txt` に書く。どれか 1 つでも 0 件でない run は捨て、run の名前と居た process を結果に名指しする。
- **始めてよい条件と確かめ方**：窓の前に、Windows の `hayamimi-eval.exe stages --help` の出力に `--only`、`--peers`、`--run`、`--cache` の 4 つが在ることを確かめる（1 つでも無ければ窓を開けない）。各 run の直前に、WSL の `/proc/loadavg` の 1 分の値が 2 未満で、Windows の `Get-Counter '\Processor(_Total)\% Processor Time'` の値が 20 未満であること。WSL の load は Windows の process を数えないので、2 つを両方 `load.txt` に書く。
- **窓の順**：vad-trace、held-tail-shadow、e-warmup の 3 つの席と E2E を先に通し、RP-74 はその後の空いた窓で打つ。窓の間は、新しい cargo の席と E2E を出さない。
- **承認や入力で止まる command**：無い。PowerShell は `-NoProfile -NonInteractive` で呼ぶ。`sudo -n` は password を訊かずに失敗するので、失敗したら run を打たずに止まる。

**残る穴**（この案の分。評議の判定を受けた設計の席が、判定を受けた手番で 10 へ移す）：

- GPU の負荷の下で `S(cpu)` が 2 倍に出る理由（NOT VERIFIED）。RP-74 は CPU の search_replay を同じ負荷の下で直に測るので、引き算に頼らずに決まる。
- R7 の WSL の run の残り（10 の H31）が探索の中に在るか（NOT VERIFIED）。RP-74 の陽性対照（足し算の式が CPU の fp32 で実測の 15% 以内に収まるか）が外れたら、見積りを使わずに評議へ戻す。
- `X` の 0.5 ms（ASSUMPTION）。RP-78 が `L_asr` で、効き目の和として受ける。
- encoder を GPU に足すと、訳 T と E と同じ GPU を取り合い、`L_tr` が延びうる（NOT VERIFIED）。RP-78 が `L_tr` を測る。
- fp32 の encoder（file は 261,057,972 bytes）が GPU の memory に常駐する。RP-22 の 14,000 MiB と、3.15 の memory の上限の行で CPU へ落とす順に認識器が入っていない。判定を受けた設計の席が 3.15 を直す。
- D の patch を当てる行は、v1.13.8 の `offline-transducer-model.cc` で 3 つの session が 1 つの設定を共有することまでを確かめた（3.4。master も `sess_opts_` は 1 つ）。patch の差分の形は実装の席が確かめる。
- 本家に送れるか：同じ分け方（offline の transducer で、`cuda` のとき decoder と joiner を CPU に置く）を sherpa-onnx の本家へ送り、入れば patch を消せる。online の側の OpenVINO の分け方（3.4）が前例になる。送るか、送って入るかは確かめていない（NOT VERIFIED）。持ち主は D の実装の席で、patch を作る手番で本家の issue と pull request に同じ分け方が在るかを数える。
- patch が効いたかは実行時には見えない（fallback の 4）。RP-77 の後段の copy の数え方が、Windows の profiler で取れない時は WSL の同じ build で数える（3.5 の転送の段落と同じ）。
- D の CPU の decoder と joiner の thread の数（今は認識器と同じ 4）。小さい graph には多すぎる疑いがあり、RP-74 の CPU の run の search_replay の thread の数と揃えて測る（ASSUMPTION）。
- encoder の CUDA の TF32 の既定の食い違い（3.4）。RP-75 の text の一致で効き目を受ける。
- この案の受入 ID は RP-74 から RP-78 である。RP-73 は並べて走る設計の席が使っている。

**決めた線**：

1. 採る形は D で、RP-74 から先へ進める。置き場は RP-78 が通るまで A のまま。
2. D の C++ の patch を持つ。実装は RP-74 の後。
3. RP-74 と RP-78 の線は上の「線」の文言で決め、結果を見て動かさない。`L_tr` の p50 の延びは 20 ms 以内とする。
4. RP-74 の run は全部同じ高い優先度で起こす（上の「起動の形」）。

### 3.6 訳の速度

会話字幕の訳は T（訳す）→ E（自然に直す）→ V（照合）の 3 段で、T と E は言語モデルに 1 回ずつ頼む。
試し 1 の実測では、CAT-Translate-3.3b の 1 回が中央値 148 ms、p95 277 ms だった（`conversation_subtitles.ja.md` 2.3）。
T と E を直列に待つと、訳の遅れはその 2 倍になる。
速度を第一にするので、次の 6 つで `L_tr` を縮める。

1. **先に T の訳を出す**：T の応答に V を掛け、通れば先の訳として自分の重ね窓と OBS の字幕頁に出す（`status: "ok"`、`stage: "t"`）。続けて E を頼み、E の応答に V を掛けて通れば同じ `id` の行を置き換える（`stage: "e"`）。E が V に落ちたら T の訳を残す。OBS の字幕頁は表示を置き換えられるので、自分の重ね窓と同じく先の訳を出して確定の訳で置き換える。V に落ちた行（`held`）と翻訳器が答えない行（`unavailable`）は、OBS の字幕頁には何も出さない（会話字幕の設計 3.4 の「相手へ」の列と同じ）。V は 1 行につき多くて 2 回で、同じ訳には 2 回掛けない。
2. **確定の訳だけを外へ出す**：**確定の訳**は、E の訳が V を通ればその訳、外れて T の訳が通っていれば T の訳とし、E の応答を照合した後に 1 回だけ決める。中継の `translation` の event は `settled`（真偽）を持ち、確定の訳を決めた event だけが真である。E の訳が V を通れば `stage: "e"` で `settled: true`、E が V に落ちれば T の訳と同じ `text` を `stage: "t"` で `settled: true` として同じ `id` に 1 回送り直す。先の訳は `settled: false` である。画面は `settled` が真の event で行を確定の形にする。Zoom と Teams への送信と SRT は確定の訳だけを使い、1 つの `id` につき 1 回送る（会話字幕の設計 3.4、3.10、3.13）。送った字幕は相手の画面で置き換えられないからである。
3. **速報（partial）の訳**：認識の速報が更新されるたびに、その文字列を T に頼み直す（re-translation）。速報の訳は `translation` の event の `of` を `"partial"` にして見分け（会話字幕の設計 3.3 の `of` に `"partial"` を足す）、`settled` は常に偽で、外の送り先には送らない。前の訳と今の訳の最長共通接頭辞だけを確定した字形で描き、残りは薄く描く。IWSLT 2026 の Pinch-AST が同じ文字単位の最長共通接頭辞の方針を取っている（S9）。速報の依頼は 1 本だけを走らせ、新しい速報が来たら走っている依頼を打ち切る（stream の応答の接続を閉じる。llama-server は接続を閉じると 1 秒以内に生成を止めて slot を空ける。`native/hayamimi-relay/tests/r8e_server.rs` の `closing_the_stream_after_the_first_token_frees_the_slot_within_a_second` が確かめる）。`cache_prompt` を真にし、伸びた文字列の前の部分の計算を使い回す。速報の訳にも V を掛け、外れたら速報の訳は出さない。
4. **揺れを数える**：1 つの発話について、字幕に出た訳の文字列が変わった回数と、一度確定した字形で描いた接頭辞が後で消えた回数を数える（RP-25）。速度のために先に出す形が、読む人に揺れとして見える量を測るためである。
5. **並べて走らせる物**：llama-server の slot を 4 にする（mic の確定、loopback の確定、速報、E）。認識（ORT）と訳（llama.cpp）は同じ GPU を時間で分け合う。負荷の下での `L` は RP-3 で測る。ライブの対の lane は T を頼む thread と E を頼む thread の 2 本を持ち、次の行の T は前の行の E の応答を待たずに頼む（RP-47）。確定の訳は音源ごとに行の着いた順に出る。T と E と速報の依頼は slot の数（4）を超えて同時に出さず、空くまで中継の中で待たせる（待った時は `translate.slot.waited` を debug で出す）。`--translate` の別の言語の lane は E が無いので 1 本のままである。
6. **llama-server の置き場**：Windows の native の CUDA 版に置く（会話字幕の設計 3.7）。model の GGUF は `%LOCALAPPDATA%\hayamimi\models\llm\` に置き、正本の WSL の `/mnt/h3-models` から写す（2.3。Windows から直に読めないため）。写した file の sha256 を同じ directory の `manifest.json` に記録する（記録で、起動の条件にしない）。hooks の `guard_commands.rs` は、`llama-` で始まる実行 file の名前を含む command を WSL の build の鮮度で refuse する（`problems` の中の `LLAMA_EXECUTABLE` の検査）。Windows の llama-server は PowerShell から起こし、WSL の shell からは起こさない（10 の H20）。

**訳の依頼の形**：中継は T と E を llama-server の `/v1/chat/completions` に頼む。T は user の message 1 つ（model の example script の形「Translate the following {Japanese|English} text into {English|Japanese}.\n\n {原文}」）、E は system と user の 2 つ（下の「E の依頼の形」）である。どちらにも`temperature` 0、`stop: ["</s>"]`、`cache_prompt: true` を付ける（S14）。
T の prompt は、空行（`\n\n`）の直後に半角空白を 1 つ置いてから原文を続ける。
同じ repository の `test.py`（https://huggingface.co/cyberagent/CAT-Translate-3.3b）がこの形で prompt を組むからで、README の例の文字列は空白を持たないが、README の形に戻さない。
数字を含む 4 文を 3 回ずつ（12 回）流すと、空白なしは q6 と f16 のどちらでも 12 回とも原文に無い数字（100、101、100%）を訳に書き、空白ありは q6 と f16 のどちらでも 0 回だった。
`temperature` 0 でも出力は決まらない。同じ 38 行を同じ依頼で 2 回打つと、出力の違う行が依頼の形によって 4〜14 行あった（10 の H66）。だから本物の model で打つ受入は、同じ入力が同じ訳を返すことを前提にしない。
chat template は llama-server が GGUF の metadata から当てる。
`/completion` に平文の `prompt` を渡す形を採らないのは、model の chat template（`<|user|>`、`<|assistant|>` の語）が入らず、CAT-Translate を測った probe（`testdata/probe1_cat-translate-3.3b.log`、`/v1/chat/completions` で測った）と違う入力を model に渡すからである。
中継が template を自分で組む形も採らない。model を替えるたびに template を写し直すことになり、写し違いを見つける手段が無いからである。
template の token の数は 2 つの形で同じなので、1 行の訳の遅れは変わらない。
`/v1/chat/completions` が `cache_prompt` と `stop` を受けることは README の `/completion` の節の記述からの読みで、確かめていない（NOT VERIFIED。T-CS-8-001 と 002 の body の検査と、R8d の速報の訳の `cache_prompt` の測りで確かめる）。
持ち主は R8c の直しで、R8e が `L` を測る前に入れる。
受入は同じ llama-server で、probe1 の 40 行の原文を中継の T に流した訳の文字列が、同じ server に `curl` で `/v1/chat/completions` を同じ body で打った訳と全行で同じであることとする（陽性対照は `curl` の側が probe1 の log の出力と同じであること。llama.cpp の版が probe の時と違うと、そこが割れることがあるので、割れた行は数を記録し、合否は中継と `curl` の一致で決める）。
陰性対照は、今の形（`/completion` に平文の `prompt`）で同じ 40 行を流し、`curl` と違う行が 1 行以上出ることである（0 行なら、この受入は 2 つの形を見分けていないので、行を足して打ち直す）。

**E の依頼の形（card の形）**：E は message を 2 つ送る。
system に T の下書きと保つ物の注を置き、user に T と同じ文（`translate_prompt_between` の形。空行の直後の半角空白を含む）を置く。
system の文は次の 2 行で、`{from}` は原文の言語名、`{to}` は訳の先の言語名である。

```text
A draft {to} translation of the user's {from} text is: {下書き}
It may contain errors. Keep the facts, numbers, proper names, negation and uncertainty of the {from} text.
```

live の対（ja と en）では、この後ろに行を足す。訳の先が ja なら `Style: {ja_style.toml の note}` と `Avoid "{literal}"; write "{natural}".` の行、訳の先が en なら `calques.toml` の `Avoid …` の行である。
対の外の言語へ向ける E（`eval` が使う `edit_system_to`）は 2 行だけで、行を足さない。
T の依頼は user の message 1 つで、system を持たない。
実装は `pipeline.rs` の `edit_system` と `edit_system_to`、`llm.rs` の `Completion::with_system` である。
E の答えは訳だけで札を持たない。`translation_of`（`edit_answer.rs`）は札の無い答えを trim して返し、札を書き返した答えを剥がす守りは残す。

この形にした理由は、probe の数である。4 文を 3 回ずつ、3 つの形で流した 36 行（`e-prompt-probe/results.tsv`）で、拒否文か指示文の復唱を訳として返した行は、指示文と見出しを user に積む形（e0）が 6 件、この形（e1）が 0 件、下書きを使わず T の形だけで訳す形（e2）が 0 件だった。
e1 は下書きの誤り（mic-26 の「100%」、mic-14 の「Suru」）も直し、原文に無い数字の混入は 0 件だった。
e0 と e1 は、指示文の置き場、見出し、下書きの置き場が同時に違うので、6 件を減らしたのがどれかは切り分けていない（NOT VERIFIED）。
probe は 1 回の実走で、`temperature` 0 でも出力は決まらない（10 の H66）ので、RP-32 の 3 run で確かめ直す。

採らない手段は 2 つである。e2 は mic-26 で意味が落ちたので採らない。V（`verify.rs`、`numbers.rs`、`verify_markers.toml`）は安全の門なので、E の形を変えた理由で緩めない。

受入は 3 本の test である。
`pipeline::tests::the_final_path_asks_t_in_one_message_and_e_in_two` は、T の依頼が user 1 つ、E の依頼が system と user の 2 つで、E の user が T と同じ文であることを確かめる。
`cs25_stages_run_in_order_t_e_v_once` は、同じことを HTTP の body で確かめる。
`eval_asks_e_with_ts_prompt_and_the_draft_in_a_system_message` は、`eval` の E も同じ形であることを確かめる。
陰性対照は、E の依頼から system を外すと、この 3 本を含む 11 本が落ちることである（実走した）。

**初回の依頼（warm-up）**：`live` は、`relay.serve.started` を出す前に、短い依頼を `--llm` に 2 本、次の順に 1 本ずつ送り、答えを捨てる。
最初の 1 回の遅さを、最初の字幕の前に払うためである。
2 本の形が違うのは、E の依頼が T の依頼と別の経路（system が付く 2 つの message）を通り、T の形だけを温めると最初の行の E が冷えたまま残るからである。

1. **T の形**：user の message 1 つ（`こんにちは。` の T の prompt）、`max_tokens` 8。
2. **E の形**：system と user の 2 つ、`max_tokens` 8。user は 1 本目と同じ文で、system は `pipeline::edit_system` が ja の原文に対して組む注（下書きは固定の `Hello.`、後ろに en 向けの calques の行が並ぶ）である。

2 本を並べて送らないのは、slot を取り合って T の依頼が遅れるからである。
2 本の待ちは合わせて最長 10 秒で、2 本目は 1 本目が使った残りだけ待つ。残りが無ければ 2 本目は送らず、`translate.warmup.failed` の `detail` が `budget` になる。
依頼ごとに 1 件の event を出す。返れば `translate.warmup.sent`（info）、返らなければ `translate.warmup.failed`（warn。`detail` は失敗の種類だけ）で、どちらも `form`（`t` か `e`）を持つ。
片方が失敗してももう片方を送り、起動は止めない。
`--llm-other` には送らない。
E の形を足すと最初の行の E が縮むことは、RP-32 の 3 run の最初の行の E の時間が 2 行目以降の E の時間に近いことで確かめる（NOT VERIFIED）。

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

**zh、ko、es の V の推量（R8i）**：zh、ko、es では、推量の不一致（原文と訳の推量の語の食い違い）を止める理由から外し、log にだけ残す（評議の判定）。否定、数、形、名前の検査は今のままで、ja と en の推量の検査も今のまま止める。
推量の表を足すと、held-out で推量で止まった行が 14 から 22 に増え、そのうちには正しい参照訳も入るからである。
log の event は `translate.hedge.logged`（info、項目は `id`、`lang`、原文と訳の推量の語）で、訳は止めずに出す。
採らない形は 3 つである。V を既定で外す形は、今は V を外す設定が無く、設定 1 つで戻せない。推量の表を足して止め続ける形は、上のとおり止まりが増える。`verify_markers.toml` の推量の語を空にする形は、`verify.rs` の `marker_mismatch` が原文の側の推量の語で止めるので、逆に止まりが増える。
戻すときは、この変更の 1 commit を `git revert` する。
merge の前に、dev の seed 0 と seed 1 の組で `hayamimi-relay vprobe`（`--out` なし、GPU は要らない）を打ち、数と否定の壊した行の止まる数が今の main 以上、参照訳の止まる数が今の main 以下であることを確かめる（RP-37）。test split の組は使わない。

**zh、ko、es の V の数の読み（R8l）**：V の数の検査（`native/hayamimi-relay/src/numbers.rs`）は、算用数字の後の倍の語を、今は日本語の `万`、`億`（`亿`）、`兆` と英語の語だけで読む。
そのため zh の `5百万`、ko の `5백만`、es の `5 millones` を 5,000,000 と読まず、原文と同じ数の正しい訳も `number` で止まる。止まった行は E を待つので、速さにも効く。
R8l は、算用数字の後に続く倍の語の列を、次の 3 つの言語で読む。
- zh：`十`、`百`、`千` と `万`、`亿` の組み合わせ（`5百万` は 5,000,000、`3千亿` は 300,000,000,000、`1.5亿` は 150,000,000）。
- ko：`십`、`백`、`천` と `만`、`억`、`조` の組み合わせ（`5백만` と `500만` は 5,000,000、`3억` は 300,000,000、`2천` は 2,000）。
- es：`mil`、`millón`（`millon`）、`millones`、`mil millones`（`5 millones` は 5,000,000、`1 millón` は 1,000,000、`2 mil` は 2,000、`3 mil millones` は 3,000,000,000）。
ko の語だけで書いた数（`오백만`）と、es の小数点が読点の数（`1,5 millones`）は R8l で直さず、10 の H51 に置く。zh の漢数字だけの数と `两` は、今の漢数字の読みで読む。
merge の前に、dev の seed 0 と seed 1 の組で vprobe を打ち、参照訳の止まる数が今の main 以下、数の壊した行の止まる数が今の main 以上であることを確かめる（RP-39）。
code の差は、倍の語の表 3 つと、算用数字の後で倍の語の列を掛け合わせる所で、40 行ほどの見込みである（unit test を除く）。
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
| `asr.final.dropped` | session の言語の文字を持たない 1〜2 字の確定文を出さなかったとき（info。8 の R3e） | `id`、`reason`（`off_script`） |
| `asr.endpoint.decided` | 確定文ごとに、どの規則で区切ったか（info。3.17、8 の R8v） | `id`、`rule`（`early`、`silence`、`max_speech`、`flush`）、`silence_ms`、`plan`（`held`、`decode`、`rest`。`rule` が `early` のときは無い）、`held_gap_ms`（持った文を使わず読み直したときだけ） |
| `asr.endpoint.held` | 早い終わりの候補が文を読んだが、文が完結していないか、声が `EARLY_MIN_SEGMENT_MS` に満たず、確定せず持った時（info。3.17） | `reason`（`incomplete`、`too_short`）、`span_ms`（読んだ声の長さ）、`min_ms`、`tail`（`noun`、`honorific`、`particle`、`interjection`、`other`）、`shadow_complete`（下の影の印） |
| `asr.model.evicted` | `--max-resident` の上限のため、最も使われていない認識器を手放したとき。新しい認識器を作る前に出す（info） | `model` |
| `asr.lang.unknown` | SenseVoice の結果の `lang` の tag を読めないとき。言語は不明として続ける（warn） | `error_code` |
| `gpu.memory.unreadable` | GPU の memory を読めないとき。予算なしで続ける（warn） | `error_code` |
| `model.fallback.substitute` | 認識器が使えず、別の認識器が答えるとき（warn） | `requested`、`used`、`reason` |
| `model.fallback.chunks` | 発話の検出器が使えず、長い音声を一定の長さで切るとき（warn） | `reason` |
| `gpu.device.unavailable` | 起動時に CUDA の device が 0 か、driver が読めない（warn） | `error_code`、`reason` |
| `<domain>.fallback.<落ち先>` | 3.15 の表の場面で落ちたとき、段ごとに 1 回（warn） | `error_code`（`HAYAMIMI_<DOMAIN>_FALLBACK`）、`stage`、`reason` |
| `<domain>.fallback.cleared` | 落ちた資源が戻って元の道へ戻したとき（info） | `stage` |

本文、音声、path は log に書かない。

#### 早い終わりの影の印

`asr.endpoint.held` は、持った文の末尾の種類（`tail`）と、名詞止めと敬称を完結として数える規則ならその場で確定していたか（`shadow_complete`）を持つ。
実際の判定（`is_complete`、`early_hold`）と確定の動きは変えない。印は、規則を切り替える前に、切り替えた時の割れと縮みを log から数えるために在る。

- **`tail`**：日本語の文を、閉じの記号と空白を除いた形で分ける。順は `honorific`（さん、様、氏）、`interjection`（さあ、はい、あの など）、`particle`（を、も、は、に、の、へ、や、など、まで、より と、完結の判定が持つ接続の形）、`noun`（漢字、カタカナ、英数字で終わる）、`other` である。`あの` が `の` で終わるので、`interjection` を `particle` より先に読む。ひらがなだけの名詞（`これ`）は `other` になり、`同様`、`模様` は `honorific` に数える。日本語以外は常に `other` である。
- **`shadow_complete`**：日本語の文が閉じの記号で終わり、`tail` が `noun` か `honorific` なら完結とみなし、声の長さの下限（`EARLY_MIN_SEGMENT_MS`）も満たす時に `true` である。生きている規則が完結とする文と、日本語以外では、生きている規則と同じ答えである。
- **置き場**：種類の判定は `endpoint.rs` の `tail_of` の 1 つで、`shadow_early_hold` と log の項目はそれを使う。`r8v_probe.rs` は判定の写しを持たず、`endpoint::is_complete` を使う。
- **文は書かない**：保持する値は種類と bool だけで、文の字は log にも `EarlyHold` にも載らない。

`asr.endpoint.decided` は、主検出器の segment が何で文になったかを `plan` で持つ。`held` は早い検出器が持った文をそのまま使った時、`decode` は segment 全体を読んだ時、`rest` は早い検出器が確定した線より後ろだけを読んだ時である。
持った文が在ったのに使わず読み直した時は、`held_gap_ms` に、持った文の範囲の終わりと segment の終わりの差を ms で載せる。使う許容は 1 窓（`WINDOW`、512 sample、32 ms）である。持った文が無い時と、使った時は載せない。
`plan` と `held_gap_ms` も判定を変えない。持った文を使い回せる率と、差の分布を log から数えるために在る。

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
主指標は `L` の p50 と p95 で、受入は「load の記録つきで固定した Python の基準以下」（RP-32）である。

1. **数え方を直す**（RP-31）：3.1 の `t_tr` の条件（`status` が `ok`、`text` が空でない）と、`id` と `t1_s` による結び方を `hayamimi-eval stream` の集計に入れる。設計を動かさない。字幕に出ない訳の時刻を `L` から外す直しで、2 から 4 の値はすべてこの数え方で読むので、最初に入れる。
② は B（R8f の V の直し）だけを進め、A は ④ の run と H6 の確認が揃った時にだけ作る。③ は作らず、RP-34 の合否列を `decode_ms` から `L_asr − decode_ms` に改め、待ちの log と GPU の記録を ④ の前に入れる（評議の判定。② は下の 2、③ は 3、④ は 4 を指す）。

2. **T が V に落ちた行の最初の訳を早める**（RP-33）：B を進める。A は作らない（評議の判定。理由は 3.18 の「採らない手段」）。作るかを開き直すのは、E0 の 1 回の p50 が E の 1 回の p50 を下回った時だけで、その時は次の 4 つを全部満たす時にだけ作る（接続を閉じると生成が止まること（H6 の確かめ）は R8e の test で通った）。2 音源が同時に話した時の T 2 本、E0 2 本、速報、E が slot 4 本に収まる（収まらなければ slot を上げて RP-22 を測る）。T が V を通った行の `L` が悪くならない。E0 にも V を掛け、閾値を動かさずに RP-35 を通す。E0 が答えなければ直列の E へ落とし、3.15 の形で event を出す。今 A を作らないのは、`feat/r8f-verify` が main に入っておらず中継の同じ関数を 2 席が触ることになり、E0 の指示が E の system の注（`edit_system`）に無い新しい prompt だからである。4 は E2E の波の中で打つので、4 の後に作る A は 07:00 に入らない。
   - **A（並べて投げる）**：T と同時に、原文だけを渡し E の指示で訳させる依頼（以下 E0）を 1 本投げる。T が V を通れば今の形（先の訳を出し、T の訳を入れた E で置き換える）で進み、E0 の接続を閉じる。T が V に落ちれば、E0 の応答に V を掛け、通れば確定の訳として出す。ただし、確定文 1 行の依頼が 2 本になり、slot 4 本（3.6 の 5）のうち E の slot を E0 が使う。2 音源が同時に話すと T 自身の待ちが延びる見込みがあり、その量は測っていない（10 の H43）。会話字幕の CS-25 の「E の依頼に T の訳が入る」が T が落ちた行で成り立たなくなるので、会話字幕の設計 3.4 と CS-25 を同じ手番で書き替える。V は 1 行につき多くて 2 回（T と E0、または T と E）のままである。
   - **B（直列のまま、落ちる行を減らす）**：依頼の形は変えない。R8f の V の直し（branch `feat/r8f-verify`）を main に入れた後に、T の訳が V に落ちた行の理由を `hold_reason` ごとに数え、正しい訳を止めている理由（会話字幕の CS-24 の材料）だけを直す。ただし、V が正しく止めた行は E を待つままで、その行の `L` は縮まない。B で残る差の大きさは R8f の直しの後の run でしか分からない。
3. **確定の復号の待ちを測る**（RP-34）：復号の順は変えない。同じ run で 4〜7 秒の発話の `decode_ms` の p50 が Python の 183 ms に対し 275 ms、7 秒以上で 248 ms に対し 440 ms だった（原因の席の打ち直し。NOT VERIFIED）。ただし `decode_ms` は `decode_segment` の `timed` が `recognizer.decode` だけを測った値で、速報の復号を待った時間を含まない。したがって RP-34 は待ちを `L_asr − decode_ms` で測る。`on_window` は feed の後に速報の復号を 1 本回す直列で、確定より先に捨てられる速報の復号は無い（発話の終わりの検出の前提は ASSUMPTION）。挙動を変えない次の 3 つは、4 の run の前に入れる。
   - 接続を閉じた依頼の生成が 1 秒以内に止まるかの probe（R8e が `r8e_server.rs` に入れ、通った）。
   - window を受けてから feed を始めるまでの待ちの log（`hayamimi` の log の event）。
   - RP-32 の各 Rust の run の前後の `nvidia-smi --query-gpu=utilization.gpu,memory.used --format=csv,noheader,nounits` の記録（`stream` が `<run>.gpu.json` に書く）。読みは各 run の直前の 1 回で、Python の基準の同じ番号の run の直前の読みと比べる。Python と Rust の対で、直前の `utilization.gpu` のどちらかが 5 % を超えるか、直前の `memory.used` の差が 256 MiB を超える対は無効にする（5 % と 256 MiB は仮の値。256 MiB は RP-6 の幅に合わせた。持ち主は R8h で、RP-32 の Rust の run の直前の読みの幅を記録し、幅が 5 % か 256 MiB に近ければ評議へ出す）。Python の基準の run の読みが `docs/results/rp32/` に無い時の扱いは 10 の H87 に置く。

   採らない形は 2 つである。速報の復号を別の thread と別の session に移す形は、認識器を CPU に置くので（3.5）2 本の復号が core を取り合う。発話の終わりで始まっていない速報の復号を捨てる形は、捨てる対象が無い。
4. **固定した Python の基準と同じ規則で Rust を測る**（RP-32）：Python の基準は `docs/results/rp32/` の `py1.jsonl`〜`py3.jsonl` と、`load1.txt`〜`load3.txt` の `py<N>` の行に固定する。Python の run は打たず、この 3 run の file と行を書き替えない。基準の 3 run は、Python と Rust を同じ機械で交互に流した時の物で、各 run の前の 1 分の load が 2 未満で、前後の load が記録されている。Rust の run は、同じ機械で 3 本（`pairs.txt` が在ればその数）を 1 本ずつ流し、各 run の前と後の `/proc/loadavg` と、中継の stdout と、`hayamimi` の stdout（`rs<N>.hayamimi.stdout.log`。`asr.endpoint.decided` の行を RP-32 の有効の線が読む）を run の file と同じ所に残す。run の間は他の席が cargo の build を打たず、各 Rust の run の前の 1 分の load が 2 未満であることを記録する（R7 の測り直しと同じ規則）。合否は Python の基準の run と Rust の run を合わせた標本で、RP-3 の期待値の式で決める。
基準の Python の 3 run の前の 1 分の load は 0.32〜0.83 である（`load1.txt`〜`load3.txt` の `py<N> before` の行）。load は CPU に置いた認識（3.5）の時間を延ばす向きに効くので、Rust の run の前の load がこの上端の 0.83 以上なら、負荷の差は Rust を遅く見せる向きにだけ効く。前の load が 0.83 より低い Rust の run は、合否は 2 未満の線で決めたうえで、その run の名前と値を結果に書き、負荷の差が Rust に有利な向きに効いた疑いとして残す（10 の H86）。
`hayamimi` は run ごとに起こし直す。同じ server に 2 回流すと `t1_s` が流れの先頭から通算になり、2 回目の `final` を発話の窓に結べないからである。各 run の前に前の run の process を止め、`memory.used` が llama-server だけの値の ±256 MiB に戻ってから直前の読みを取る。Rust の `hayamimi` が GPU を持ったまま次の run の読みを取ると、対が構造的に無効になるからである（評議には出さない。読みを取る時を決めただけで、幅は変えない）。

   **RP-32 の報告**：速度の合否は、3.1 の終端不正の例外のとおり行ごとの線で数えた値で決める。報告には、同じ run を全標本 `min_silence` の線で数えた対の比較の数（Rust が Python 以下の発話の数 / 比較集合の発話の数）と、行ごとの線で数えた同じ数を並べて書き、行ごとの線の数から閾値（RP-3 の式）を引いた余裕も数で書く。余裕が 0 の時は、1 発話が入れ替わると落ちる位置に在ると書く。2 つの線の間で有効から外れた Rust の標本の数と、その `rule` ごとの内訳も書く。品質の数は速度の門の外に並べて記録し、合否に入れない。並べる数は、`finals` が 2 以上の標本の割合（枠の全標本で数える。線で絞ると割れた標本が抜けて小さく出るからである。3.17 の「RP-66 と RP-67 の数え方」の 3）、ja の CER（run の全標本の `cer_dist` の和 ÷ `cer_denom` の和）、「final なし」の標本の数で、Python の基準と Rust の値を並べる。

   採らない形は 3 つである。早い detector の無音を `min_silence` まで上げて線を 1 本に揃える形は、早い終わりの知らせ（3.17 の方針 1）を A の区切りと同じ遅さにし、R8v が縮める `d` を戻す（評議の判定、VETO）。RP-32 の全標本の線を `EARLY_MIN_SILENCE_MS` に下げる形は、3.1 の例外の理由のとおり、`silence` の規則の標本の終端不正を有効に入れる。線の値を test や結果の file に数で書く形は、早い detector の定数を変えた時に片方だけが残るので、`EARLY_MIN_SILENCE_MS` から取る。

**品質を下げない条件**（RP-35）：2 の B と、作る時の A のどちらでも、字幕と外の送り先に出る訳は全部 V を通った物で、V の検査（形、数、名前、否定）を緩めない。ja→en と en→ja の held-out（会話字幕の設計 9）の chrF と、V が止めた行の数が、R8h の前の main の値から下がらず増えない（同じ server、同じ model）。FLEURS の ja→en の chrF（RP-13 の en の行）も R8h の前の値以上である。3 の待ちの log は RP-1 の ja の CER と RP-5 の速報の遅れを悪くしない。

**R8f の V の直しとの関係**：2 の B は R8f の V の直しの上に乗り、A を採る時も、A の効き目は R8f の直しの後の run で測る。V の直しで T が V を通る行が増えれば、2 の対象の行は減るからである。R8h の 2 の席は `feat/r8f-verify` が main に入るまで、中継の `pipeline.rs` の V の呼び出しの所を書き替えない（同じ関数を 2 席が触ると衝突する）。

**RP-32 の dir の読み方**：対の数の記録 `pairs.txt` は、file の全体から前後の空白と改行（Rust の `str::trim` が除く物）を除いた残りが算用数字だけの時に限ってその数として読み、`2\n`、` 2 `、`2\r\n` は 2 と読み、残りが空か、算用数字でない字（`2 対`、`two`、`+2`）を含む時は数が読めないとして落とす。
dir（`docs/results/rp32/`）の run と load の file は、対の番号 `<N>`（1 から順）ごとに Python の基準の run が `py<N>.jsonl`、Rust の run が `rs<N>.jsonl`（どちらも `stream --out` の出力）、Rust の run の events が `rs<N>.events.jsonl`、`hayamimi` の stdout が `rs<N>.hayamimi.stdout.log`（`docs/results/r8h_verify/` の `e2e2.hayamimi.stdout.log` と同じ形）、load の記録が `load<N>.txt`（1 行が `<run> before|after <時刻> </proc/loadavg の 1 行>`）とし、これは `feat/rp32-test` の `fc66c1d` の `native/hayamimi-eval/tests/support/rp32.rs` の名前と同じである（`pairs.txt` と `rs<N>.hayamimi.stdout.log` を読む code はその commit にまだ無いので、読む code が上の規則に合わせる）。
対の番号は Python の基準の run と Rust の run を同じ番号で組む札で、流した順を表さない。Rust の run を流し直す時は名前が `rs<N>.` で始まる file と `load<N>.txt` の `rs<N>` の 2 行だけを置き換え、`py<N>` の file と行は残す。2 対の dir は `py1`、`py2` を基準に読む（`py3` を使わない規則は対の数で決まり、結果を見て選ばない）。
判定は流した順を確かめない。Python の run が先頭か、Python と Rust が交互かは、基準を固定した dir では意味を持たないからである。Rust の `final` の `id` を `rs<N>.hayamimi.stdout.log` の `asr.endpoint.decided` の `id` と結び、標本を閉じた `final`（標本に結んだ最後の `final`。`t_final_us` を決める物）の `rule` で線を選ぶ。結べない `final` が 1 件でも在れば、その run と `id` を挙げて値を出さずに落ちる。欠けた `rule` を `silence` と読む形は、欠けを既定で埋めることになるので採らない。

**R7 と R7b との関係**：R7 は認識の段の時間と置き場を持ち、R8h の 3 は確定の復号の前の待ちを測るだけで、復号の順を変えない。R7 の H10（確定の解き直しを省く）は同じ loop を触るので、H10 と R8h の 3 の待ちの log は同じ席が続けて打つ。R7b は日本語の streaming の認識器を測るだけで、`hayamimi` の経路を替えない（10 の H38）。streaming の認識器を採ると、速報は解き直しでなくなり、3 が測る待ちの形も変わる。採るかは E2E の後に評議が決める。

**順と置き場**：1 は W3 の空いた席がすぐ打ち、E2E-1 と E2E-2 も 1 の後の道具で測る。2 の B は R8f の V の直しの席が持つ。3 の待ちの log と GPU の記録は W3 の空いた席が 4 の前に入れる。2 の A は作らない（上の 2 の開き直す条件を満たした時だけ設計の席へ戻す）。4 の Rust の run は E2E の波で打ち、E2E-1 の `L` の判定も 4 の run で決める（仮の置き場。持ち主は R8h で、E2E の波の 1 時間に Rust の 3 本が収まらなければ 2 本にし、その数を記録する）。
4 の run の後の訳の段の詰め（部分訳の使い回し、推測デコード、RP-32 の判定の直し、run の間の port の待ち）は 3.18（R8w）が持つ。
2 対の数の記録は、run の file と同じ dir（`docs/results/rp32/`）の `pairs.txt` に、対の数の算用数字 1 つ（`2`）だけを 1 行で書く。
`pairs.txt` が無い dir は 3 対として読み、在る dir はその数の対を読む。
`pairs.txt` の数と在る対の数が違う時、数が読めない時、`1` の時、RP-32 の実走の test（`rp32_alternated_runs_under_the_same_load_beat_python`）はその理由を挙げて落ちる。
対の数を run の名前や `load<N>.txt` から数えて済ませないのは、3 対目を流し損ねた dir と、2 対で止めると決めた dir が同じ顔になるからである。

### 3.17 発話の終わりの判定（R8v）

目標は `L` の p95 を 1000 ms 以下にすることである。
`L` は 3 つに分けて読む。
**終わりの待ち** `d` は `L_asr − L_dec`（声の終わりから、VAD が区切って復号を始めるまで）、**復号** は `L_dec`（`final_latency_ms`）、**訳** は `L_tr` である。
E2E-1 と E2E-2 の記録（main `7ea558f` の build、`/home/fukugan/.cache/hayamimi-e2e-20261015/run3/`）を、訳の取れた有効な標本で分けると次のようになる。

```
jq -s '[.[]|select(.t_final_us!=null and .t_tr_us!=null)
  |{d:(((.t_final_us-.t_end_us)/1000)-.final_latency_ms),
    dec:.final_latency_ms, tr:((.t_tr_us-.t_final_us)/1000)}]' e2e1.jsonl
```

| run | n | `d` p50 / p95 | 復号 p50 / p95 | 訳 p50 / p95 | `L` p50 / p95 | `L` ≤ 1000 ms の標本 |
|---|---|---|---|---|---|---|
| e2e1（ja→en） | 93 | 881 / 1355 | 135 / 234 | 291 / 1034 | 1415 / 1871 | 6 |
| e2e2（en→ja） | 70 | 448 / 671 | 306 / 422 | 227 / 285 | 970 / 1303 | 38 |

p95 は nearest-rank、`d` の最小は ja 428 ms、en 361 ms である（2026-10-15 14:1x に打った）。
ja では `d` が `L` の 6 割を占め、`--min-silence` の 300 ms の 3 倍近くになっている。
ただし ja の訳の p95 は、それだけで 1034 ms ある（T が V に落ちて E を待つ行。93 行中 14 行が 500 ms を超える）。
したがって終わりの判定を直しても、ja の `L` の p95 は 1000 ms に届かない。
この節は `d` を縮める所までを持ち、残りの差は 10 の H69 に置く。

#### 待ちが `min_silence` より長い理由

sherpa-onnx の Silero は、確率が `threshold` を超えると声の始まりとし、確率が `neg_threshold`（既定は `threshold − 0.15`）を下回った時から無音を数え始め、`min_silence` 続いたら区切る（S25）。
今の `threshold` は 0.5 なので、終わりの側の線は 0.35 である。
ja の声の後ろには、確率が 0.35 と 0.5 の間に留まる区間が長く続く。
`docs/results/vad-threshold-20261015/` の probe では、区切りの終わりから manifest の `end_sample` を引いた差が、ja で p50 524 ms、p95 877 ms、en で p50 78 ms、p95 250 ms だった。
この差は、detector が声の終わりを遅れて言う量と、`end_sample` の置き方の誤り（3.1 の起点の確かさ）の和である。
起点の確かな発話だけで見ると、差は ja の確か 4 発話で 100、108、300、706 ms、en の 15 発話で 26〜250 ms（p50 78 ms）である。
ja の切れの発話では、区切りの終わりが clip の終わりより 326〜665 ms 後ろ、すなわち声の後のつないだ無音の中に在る（発話 11、13、14、20、21、22。発話 19 は 1,818 ms 前で、発話 10 は区切りが触れない）。
`L` の遅い ja の発話 16 と 22（`83830af` の build の RP-32 の 3 run、15 標本の中央値で 1,509 ms と 1,367 ms。記録の在り処は 10 の H90）は、確かと切れの発話である。
したがって確かと切れの 12 発話では、ja の待ちが `min_silence` より長いのは、主に detector が無音の中で声を言い続ける量で、`end_sample` が早すぎるからではない。不確かの 11 発話には言えない。この読みは H93 の結果が出るまで暫定である。
`threshold` を 0.6 に上げると ja の差の p50 は 288 ms に縮んだが、始まりも同じ値で上がるので、発話の始まりが 256 ms 遅れた。
1 つの値が始まりと終わりの両方を決めているからである。
`neg_threshold` は sherpa-onnx の C++ の設定には在るが、C の API（`c-api.h` の `SherpaOnnxSileroVadModelConfig`）と Rust の crate 1.13.8 の `SileroVadModelConfig` には無い（crate の source を `rg -n neg_threshold` で 0 件、同じ範囲の `threshold` は 1 件以上。crates.io の最新も 1.13.8）。
`--min-silence` を 0.20 にした run では、`d` の p95 が ja で 1355 ms から 1247 ms、en で 672 ms から 600 ms に縮み、`L` の p95 は ja で 1872 ms から 1849 ms、en で 1303 ms から 1277 ms に縮んだ。
`d` は無音を短くした分に応じて縮み、`L` の p95 に出たのはその約 4 分の 1 だけである。
`L` の p95 は `d` の遅い標本だけでなく、訳と復号の遅い標本でも決まるからである。
2 つの run は負荷を揃えて交互に流した対ではないので、縮みの量は断定しない（10 の H68）。
したがって終わりの線（0.35 から 0.50）と無音の長さ（0.30 秒から 0.12 秒）の両方を動かし、`L` の p95 は `d` とは別の受入（RP-67）で見る。

#### 方針

区切りを 2 段にする。
始まりと、迷った時の区切りは今の detector（threshold 0.5、`min_silence` 0.30 秒）のまま残す。
その上に、終わりだけを早く言う 2 本目の detector を足し、言った時点の文が完結していれば、そこで確定する。

1. **早い終わりの detector**：同じ Silero をもう 1 つ開き、threshold 0.65（終わりの線は 0.50）、`min_silence` 0.12 秒、最短の発話 0.25 秒で、同じ window を同じ順に食わせる。使うのは「声が終わった」という知らせだけで、その detector の始まりと segment の中身は使わない。知らせは、今の detector が声の中に居る間だけ候補として扱う。
2. **候補で解く**：候補の時点で、今の detector の声の始まり（preroll を含む）から今の sample までを、確定と同じ経路（振り分け、認識、ITN、句読点、置換辞書）で解く。
3. **完結の判定**：ja は、句読点を付けた文が `。`、`？`、`！` で終わり、その前の語尾が文末の形の表に当たり、接続の形の表（`が`、`けど`、`て`、`で`、`し`、`から`、`ので`、`のに` ほか）に当たらない時に完結とする。en は、認識器の文が `.`、`?`、`!` で終わり、最後の語が続きの語の表（`and`、`but`、`so`、`because`、`the`、`a`、`to`、`of` ほか）に無い時に完結とする。ja と en の外の言語は判定せず、候補を使わない。表は `native/hayamimi-asr/src/endpoint.rs` に定数で置き、行は test-designer の席が case 行で固める。
4. **完結なら確定する**：候補で解いた文を `final` として出し、`t1_s` は候補の時点の sample にする。今の detector（以下 A）には何も打たない。流れの途中で `flush` も `reset` も打たず、A は同じ window を食い続ける。route は候補の sample を**確定の線**として持つ。話者の音声（`Final.speaker_audio`）は、A の声の始まりから確定の線までの `history` の sample から取る（今は segment の sample から取っている。`pipeline.rs:680`）。
   - 確定の後に A の segment が出てきたら、確定の線より前の部分は確定済みとして解かない。
   - 確定の線より後ろ、その segment の終わりまでの間に、早い detector（以下 B）が声の始まりを読んでいれば、同じ発話の中で声が続いたとみて、確定の線から segment の終わりまでを 1 つの segment として `process` に渡す。これが B だけが作る割れで、RP-66 は `finals` が 2 以上の標本として記録し、本物の文の切れ目、文の途中、尾の雑音に仕分けて、文の途中の切断の率を合否に使う（3.17 の「RP-66 と RP-67 の数え方」の 8）。
   - B が声の始まりを読んでいなければ、その segment は final を出さずに捨てる。
   - 確定の線より後ろで始まる A の segment は、次の発話として今のとおり解く。
   - A の `flush` を流れの途中に打たないのは、sherpa-onnx の `Flush()` が buffer を空にして `start_` を -1 に戻すのに、model の `triggered_` と `temp_end_` を残すからである（S25 と同じ file。固定している版 v1.13.8 と master で `voice-activity-detector.cc` の sha256 が同じ）。そのまま声の続きの状態で無音を食わせると、次の区切りで `buffer_.Get` に渡す長さ（buffer の大きさ − `min_silence` の sample）が負になり、process ごと abort する（機構は評議の席が source を読んだ読みで、この席は v1.13.8 の `Flush()` と `Reset()` の本文までを読んだ）。手番 1 の probe の abort（`Invalid n: -2752. size: 2048`）は 2048 − 4800 と一致する。`reset()`（Rust の crate の `VoiceActivityDetector::reset`。model の状態も戻す）は buffer の音声も捨てるので、流れの途中では使わない。
5. **完結でなければ待つ**：解いた文と、解いた範囲の終わりの sample を持っておき、今の detector の規則で区切れるのを待つ。区切れた segment の終わりが、持っておいた範囲の終わりから 1 window（512 sample）以内で、その間に早い detector が声を読んでいなければ、持っておいた文をそのまま `final` にし、解き直さない。それ以外は今のとおり segment を解く。
6. **log**：`final` ごとに `asr.endpoint.decided`（info）を 1 件出す。項目は `id`、`rule`（`early`、`silence`、`max_speech`、`flush`）、`silence_ms`（区切りに使った無音の長さ）である。`/events` の `final` の形は変えない（RP-12 が key と型を Python の fixture と比べるからである）。
7. **外せる形**：`hayamimi` の `--endpoint early|silence`（既定は `early`）で、2 本目の detector を開かない形に戻せる。`/config` と画面には出さない。
8. **変えない物**：速報（draft）の周期、中継、画面、`t_end` の定義、Silero の threshold 0.5、`--min-silence` の既定 0.30 秒。

早い detector の threshold を 0.65 にするのは、終わりの線（`threshold − 0.15`）を 0.50 にするためである。
始まりはその detector から取らないので、0.6 の probe で出た始まりの遅れはこの形では起きない。
完結の判定を挟むのは、0.12 秒の無音で区切ると、文の途中の息継ぎで行が割れるからである（0.6 の probe で、0.30 秒の無音でも割れが 4 から 6 に増えた）。
完結した文でも、声が 1000 ms に満たない候補では確定しない（`vad.rs` の `EARLY_MIN_SEGMENT_MS`）。
声の長さは、確定の線（線が無ければ A の声の始まり）から候補の sample までで測り、足りない候補は完結でない候補と同じに、解いた文を持っておいて A の無音を待つ（方針 5）。
短い候補を確定しないのは、B が語の中の短い無音（促音の前後など）でも「声が終わった」と知らせ、その時点の 0.4 から 0.5 秒の切れ端が語尾の表（`た`、`う`、`る` ほか）に当たると完結と判定されて、語の途中の切れ端が 1 つの文として確定し、続きが別の行に割れるからである。
`docs/results/rp32/rs1` から `rs3` の 3 回の記録では、`rule` が `early` の `final` のうち `t1_s − t0_s` が 1000 ms に満たない物は run ごとに 9、10、10 件で、文面は「一分間で降った。」「言ったろう。」「はい。」の 3 種だけである（1000 ms 以上 1500 ms 未満は「アピアはサモアの首都です。」の 1 種）。
持っておく代わりに、1000 ms に満たない発話は早い終わりの利得（A の無音 0.30 秒と B の無音 0.12 秒の差、約 0.18 秒）を失う。
持った候補は `asr.endpoint.held`（info。`reason`、`span_ms`、`min_ms`）を 1 件出す。`reason` は、文が完結していないときが `incomplete`、完結していて声が短いときが `too_short` である。
`incomplete` を数えるのは、完結の判定がどれだけの候補を持っているかを log で数えてから、語尾の表と名詞止めの扱いを決めるためである。語尾の表（`endpoint.rs` の `JA_FINAL`）に無い字で終わる名詞止めは完結と読まれず、`incomplete` に入る。この数が出るまで、名詞止めを早い終わりの完結に数える形は採らない。
`silence`、`max_speech`、`flush` の規則は長さを見ない。
効いたかは RP-32 の `finals` が 2 以上の標本の割合と `L` の p50、p95 で測る。
採らない形は 2 つである。B の無音を 0.12 秒より上げる形は、早い終わりの知らせを A の区切りに近づけ、R8v が縮める `d` を戻す（3.16 の採らない形と同じ評議の判定、VETO）。
語尾の表から `た`、`る` を外す形は、文末が `た`、`る` の完結が全部外れ、秒数の余裕が薄い発話の確定が A の無音まで遅れる。

候補で解いた文を `final` に使ってよいのは、候補で解く経路が確定と同じだからで、速報（ITN と句読点を通さない文）とは別物である。
完結でない候補の文を持っておくのは、待った後に同じ音声を解き直す時間（復号の p50 で ja 135 ms、en 306 ms）を `L` から外すためである。
Silero の 1 回は 30 ms 以上の塊で CPU の 1 thread 1 ms 未満なので（S8）、2 本目の detector の重さは `L` に対して無視できる見込みである（NOT VERIFIED。R8v の手番 1 の `vad_ms` で測る）。

#### 変わる物と、それを読む者

数えた範囲は `native/` の `*.rs`（`target/` を除く）で、2026-10-15 の `8f3468b` である。

| pattern | 結果 | 陽性対照 | 扱い |
|---|---|---|---|
| `rg -c 'min_silence\|min-silence' --type rust -g '!target' native` | 22 file | `vad.rs` に 7 件 | `--min-silence` の意味（今の detector の無音）は変えない。eval の 3.1 の終端不正の式は、RP-66 と RP-67 が全標本を 0.12 秒で、RP-32 が Rust の標本ごとに `rule` で選んだ線で数える（3.1 の例外、10 の H70、H86） |
| `rg -c 'VadSettings\|Sensitivity\|retune_vad' --type rust -g '!target' native` | 11 file | `vad.rs` に 19 件 | `/config` の `retune_vad` は 2 本の detector を同じ時に作り直す（`Vad::rebuild`） |
| `rg -n 'Vad::open' --type rust -g '!target' native` | 3 か所（`vad.rs`、`pipeline.rs`、`hayamimi-eval/src/der_run.rs`） | `vad.rs` に 1 件 | DER の run も同じ区切りを通るので、RP-15 を打ち直す（10 の H73） |
| `rg -n '\.detected\(\)\|draft\.observe' --type rust -g '!target' native` | `vad.rs` に 2 件、`pipeline.rs:545` に 1 件 | `vad.rs:168` | 速報の時計は今の detector の `detected` を読み続ける |
| `rg -l 'partial_event\|"partial"' -g '!target' -g '!*.md' native mobile/hayamimi_core/lib` | 38 file | `native/hayamimi/src/session.rs` | wire 形式を変えないので触らない |

#### 案の比べ

案は 3 つである。
見積りは、上の表の値から、probe で測れた差を引いた足し算の見込みで、確かめていない（10 の H68）。

| 案 | 効きの見積り | 根拠 | 代償 | 変える所 | 戻し方 |
|---|---|---|---|---|---|
| 1（推す）：2 段の区切りと完結の判定 | 完結で確定した標本で、`d` が ja で 400 ms 前後（終わりの線を 0.35 から 0.50 へ上げる分の p50 236 ms、無音の 300 ms から 120 ms への 180 ms）、en で 180 ms 前後縮む。全標本がこの形で確定すると、`L` は ja で p50 約 1000 ms、p95 約 1500 ms、en で p50 約 800 ms、p95 約 1120 ms | probe の 0.6 と 0.7 の終わりの差（`docs/results/vad-threshold-20261015/`）。Phoenix-VAD は文の完結を判定して、完結なら 400 ms、未完なら 1000 ms の無音で区切る 2 つの時間を置き、実音声の文単位の正解率が完結 0.800、未完 0.838（同じ表の Smart Turn v3 は 0.704 と 0.623）（S27）。認識と終わりの判定を 1 つの model で持つと、別々の model より終わりの遅れが中央値で 120 ms（30.8%）、90 パーセンタイルで 170 ms（23.0%）縮み、WER は悪くならない（S28） | 完結の判定が外れると行が割れる。完結でない候補の分だけ復号が 1 回増え、同じ core を速報の復号と取り合う（3.16 の 3）。候補の後に続いた音声は次の発話になる | `native/hayamimi-asr/src/vad.rs:13-17`（定数）、`:140-211`（`Vad` が 2 本目を持ち、`accept` と `rebuild` が両方を扱う）、`native/hayamimi-asr/src/pipeline.rs:533-548`（`feed` で候補を解く）、`:639-705`（持っておいた文を使う）、新しい `native/hayamimi-asr/src/endpoint.rs`（完結の表）、`native/hayamimi/src/session.rs:118-132`（log）、`native/hayamimi/src/args.rs:46`、`:186`、`:206` の並び（`--endpoint`） | `--endpoint silence` で起こし直す。code は R8v の commit を `git revert` |
| 2：先読みだけ（区切りは今のまま） | 候補の時点で解いた文を中継へ先に渡し、T を走らせておき、`final` の文が同じなら解いた結果と T の答えを使い回す。復号と T の時間（ja の p50 で約 135 ms と約 150 ms）が `L` から外れるが、`d` は縮まない。ja の `L` の p95 は `d` の p95 の 1355 ms を下回れない | 部分の認識結果で先に応答を取りに行き、最終の結果が同じなら使い回す形で 200 ms 縮み、end-to-end の判定は無音で決める形より同じ先読みの率で 100 ms 速い（S29）。部分の発話から全体を予測して先読みする形は、外れた予測の分だけ計算が増える（S30） | 中継の変更（`translate_partial` の経路と、原文の sha256 で引く答えの持ち置き）。外れた先読みの分だけ slot（3.6 の 5）と GPU を使う | `native/hayamimi-relay/src/pipeline.rs:554-600`、`native/hayamimi-relay/src/line.rs:145-152`、`native/hayamimi-asr/src/pipeline.rs:533-548` | 2 つの crate の commit を `git revert` |
| 3：streaming の認識器と終わりの判定を 1 つにする | 案 1 の根拠の S28 と同じ桁の縮み（中央値 120 ms、90 パーセンタイル 170 ms）に、確定の復号が streaming に重なる分が足される見込み。ja の streaming の認識器の誤り率は測っていない | S28。transducer の出力の遅れが終わりの判定を誤らせ、終わりの語の token と frame ごとの声の判定の補助の網で直す形（S26。数は abstract に無い） | 認識器を替える。ja の streaming の認識器の CER が今の ReazonSpeech 以下であることが前提で、それは R7b が測るだけの段にある（10 の H38）。速報、確定、清書の経路が全部変わる | `native/hayamimi-asr/src/` の route、decode、`partial.rs`、`pipeline.rs` の全体 | 今の offline の経路を残し、設定で切り替える。採るなら評議の判定が要る |

**推す案は 1 である。**
3 つのうち、`final` の時刻（`d`）を ja と en の両方で前へ動かすのは 1 と 3 だけで、3 は認識器を替えるので誤り率の前提が測れていない。
1 は中継と wire 形式と画面を変えず、`--endpoint silence` で起こし直せば今の形に戻る。
2 は 1 と重ねられるが、`d` を縮めないので、単独では ja の p95 に効かない。
1 の 5 は、2 の「解いた結果の使い回し」のうち `hayamimi` の中で閉じる部分だけを先に取り込んだ形である。

#### 採らない手段

- **Silero の threshold を 0.6 か 0.7 にする**：ja の行が割れ（4 から 6、8）、発話の始まりが 256 ms 以上遅れる（`docs/results/vad-threshold-20261015/`）。
- **`--min-silence` を 0.20 にする**：p95 が 23〜26 ms しか縮まず、割れた行が増える。
- **`t_end` の定義を変える**：測る物差しを結果に合わせることになる（3.1）。起点を不確かの発話で音量の後ろの終わりへ動かしても、ja の Rust の `L` の p95 は 1,481〜1,509 ms の間にしか動かず（6 の RP-73 の行）、1000 ms の線に届く手にならない。
- **認識を GPU に移す**：同じ精度の対で CPU int8 の方が速い（3.5、R7）。
- **小さい認識器に替える**：誤りが増える。速度の予算の中で品質を落とさない（1.1）。
- **sherpa-onnx の C の API に `neg_threshold` を足す patch を当てる**：自分で build している（3.12）ので当てられるが、版を上げるたびに patch を当て直す手間が残る。2 本目の detector なら同じ終わりの線を patch なしで得られる。
- **完結の判定なしに、早い終わりで毎回確定する**：0.12 秒の息継ぎで行が割れる。
- **音声だけで完結を判定する model（Smart Turn v3）を足す**：Silero の後に置く形で 23 言語（ja を含む）を持ち、CPU で 10〜65 ms と軽い（S31）が、実音声の文単位の正解率は完結 0.704、未完 0.623 で、文字の規則より良いとは言えない（S27）。ja での正解率も測られていない。
- **言語モデルで完結を判定する（Phoenix-VAD の形）**：判定 1 回に 140 ms 掛かり（S27）、訳の T と同じ GPU を取り合う。

#### 受入、slice、残る穴の在り処

受入は RP-66（6 の表）、test の名前は `rp66_early_end_shortens_the_wait_without_splitting_utterances`（7 の表）、slice は R8v（8 の表）である。
主指標の `L` の p95 を 1000 ms 以下にする受入は RP-67 で、R8v だけでは通らない（10 の H69）。

RP-66 と RP-67 の数え方は次のとおりに決める。

1. **`d` の標本**：RP-66 の `d` は、訳の有無を問わず有効な標本（`final` が在り、終端不正でない標本）で数える。`d` は訳を待たない量だからである。p50 も p95 と同じ nearest-rank（n 個を小さい順に並べた ceil(0.5 × n) 番目）で取る。この数え方で今の main の記録の `d` は ja が p50 865 ms、p95 1341 ms（n=100）、en が p50 457 ms、p95 706 ms（n=75）である（`final` の在る標本で `L_asr − L_dec ≥ 120 ms` の物を `jq` で数えた）。3.17 の冒頭の表の値（ja 881、en 448）は訳の取れた標本の値で、RP-66 の陽性対照には使わない。
2. **「final なし」**：3.1 の定めのとおり、その標本の窓に `final` が 1 件も結ばれない標本、すなわち標本の記録の `t_final_us` が null の物とする（e2e1 では 15 件）。`finals` の値は `final` の数の記録で、「final なし」の判定には使わない。
3. **RP-67 の有効の線**：RP-67 も `min_silence` を 0.12 秒として数える（3.1 の例外）。R8v の build で早く確定した標本は `L_asr − L_dec` が 0.30 秒より短くなりうるので、0.30 秒の線で数えると速い標本ほど有効から外れ、`L` の p95 が実際より悪く出るからである。線で偏るのは割れの数のほうで、早く確定して割れた標本が有効から外れると割れの数が小さく出る。そのため RP-66 と RP-67 の `finals` が 2 以上の標本の数と「final なし」の標本の数は、有効の線で絞らず、枠（発話の数 × 回の数）の全標本で数える。`L` は訳の取れた有効な標本だけで数え、訳の取れない有効な標本の数を同じ結果に記録する。この数が対の main より増えると、遅い行が母数から抜けて `L` の p95 が小さく出る向きに効くからである。RP-67 の test は 0.30 秒でも数えて並べて記録する。
4. **±60 ms の帯の両端**：差がちょうど 60 ms の時は帯の内とする（`|差| ≤ 60`）。
5. **ja の限度 1050 ms を置き直す時**：置き直す席は、新しい値と理由を `docs/results/r8v/probe.md` に書き、同じ commit で 6 の RP-66 の行の値を書き換える。値は次の式で 1 回だけ決める。直した probe（8 の R8v の手番 1）で、lat-ja の全 23 発話の検出の遅れ（完結の候補が出た発話はその最初の時刻、出なかった発話は A の検出の時刻。fallback を含む）の p95 を P_B、同じ probe の A だけの検出の遅れの p95 を P_A とし、今の main の記録の `d` の p95（1341 ms。3.17 の「RP-66 と RP-67 の数え方」の 1）との差 1341 − P_A を復号の前の待ちの補正として足し、限度 = P_B +（1341 − P_A）を 10 ms 単位に切り上げる。限度が 1341 ms 以上になる時は、R8v が ja の p95 を縮めないので手番 2 へ進まず評議へ出す。T の行（T-RP-66-002〜005）の境界の値は、その commit の後に test-designer の席が新しい値で書き直し、`rp66.rs` の限度の値は D の行と同じにする。書き直すまで RP-66 の手番 3 の run は打たない。
6. **今の main の記録の置き場**：今の main（`7ea558f` の build）の記録は、既に git の中の `docs/results/e2e/e2e1/e2e1.jsonl` と `docs/results/e2e/e2e2/e2e2.jsonl` に在り、repo の外の run3 の同じ名前の file と sha256 が一致する。test はそれを読み、写しを作らない。R8v の新しい run は `docs/results/r8v/` の下に、対の番号 `<N>` ごとに `main<N>.e2e1.jsonl`、`r8v<N>.e2e1.jsonl`、`main<N>.e2e2.jsonl`、`r8v<N>.e2e2.jsonl` と load の記録 `load<N>.txt`（RP-32 と同じ 1 行の形）で置き、run の生の残り（stdout、pid、gpu の csv）は git の外の run の作業 dir に残す。
7. **2 本の run の合わせ方**：RP-66 の `d` の合否は、言語ごとに R8v の 2 本の run の有効な標本を合わせた集まりで p50 と p95 を取る。陽性対照の main も 2 本を合わせて取る。品質の合否（文の途中の切断の率、「final なし」の数、訳の取れない有効な標本の数、ja の CER）は、合わせた 2 本の R8v と合わせた 2 本の main で比べる。run ごとの値は記録だけにする。
8. **B だけが作った割れの仕分け**：B だけが作った割れ（A は 1 つの segment なのに、確定の線で 2 つ以上の final になった物）の 1 件ごとに、確定の線の前の final の文（前の文）が参照の文のどこで終わるかを、字の並びの合わせ（ja は NFKC の後に空白と句読点を除いた字、en は小文字にして句読点を除いた語の並び、編集距離の最小の合わせ）で決め、次の 3 つのどれか 1 つに入れる。**本物の文の切れ目**：合わせた終わりの位置の直後（±1 字、en は ±1 語）に、参照の側の文の終わりの印（ja は `。`、`？`、`！`、en は数え方の 9 の印）が在る。**尾の雑音**：確定の線の後ろの final の文（後ろの文）を同じ形で除いて空か、後ろの文のどの字（en は語）も、参照の文のうち前の文が合わなかった残りに合わない。**文の途中**：上の 2 つのどちらでもない。1 つの割れが本物の文の切れ目と尾の雑音の両方に当たる時（参照の文の終わりで割れ、後ろの文が参照のどの字にも合わない時。参照の最後の文の末尾で割れた形）は尾の雑音に入れる。後ろの文が参照の言葉を 1 つも運んでおらず、利用者に見える害は文の切断ではなく、雑音の final が 1 つ増えることだからである。仕分けた 3 つの数を並べ、文の途中の数を割れの害の数とする。訳の側の害は、RP-35 と同じ型の chrF で見る。割れた標本の訳（前の文の訳と後ろの文の訳をつないだ物）を、同じ音声の A の 1 つの final の訳を参照として chrF を取り、記録する（参照訳ではなく A の訳との近さなので、合否に入れず、上限を置くかは評議が決める）。
9. **en の参照の文の終わりの印**：en の参照（`lat-en` の元の LibriSpeech と、AMI）は句読点を持たないので、参照の文の終わりは、同じ音声を A だけで解いた 1 つの final の文（Parakeet の句読点の付いた文。今の main の build）の `.`、`?`、`!` の位置を、数え方の 8 の合わせで参照の語の並びへ写して決める。A の文を物差しにするので、この印は認識器の句読点の誤りをそのまま含む（ASSUMPTION）。手の確からしさは ja で測る。ja の割れを、参照の `。` による仕分けと、同じ A の final の `。` による仕分けの 2 通りで行い、一致した件数と一致しなかった件数を並べて記録する。一致の上限は、一致しない件数が割れの数の 5 % 以下（割れが 20 件未満なら 0 件）である。上限を超える時は、en の仕分けを合否に使わず評議へ出す。en の仕分けは参考値で、一致の上限を満たしても自動では合否に入れない。
10. **発話の終わりの遅れ（probe の数え方）**：発話ごとの遅れは、その発話に結ばれた**最後の final**（B だけの割れの後ろの文を含む）の検出の時刻から manifest の `end_sample` を引いた値で取る。「最初の完結の候補か A」の時刻で取る形は、割れた発話で後ろの文を待つ時間を数えず、遅れを小さく見せるので使わない。A の segment が 1 つも触れない発話は遅れの母数から外し、その数を別に書く（10 の H77）。p50 と p95 は nearest-rank で、比べる相手は同じ数え方の A だけの値である。評議の数えでは、lat-ja が最後の final で 508 / 880 ms、A だけで 824 / 1177 ms、held-out の ja が 428 / 1340 ms と 532 / 1724 ms、lat-en が 186 / 550 ms と 378 / 550 ms だった（転記。この席は打ち直していない）。数え方の 5 の P_B と P_A も、この最後の final の数え方で取る。この遅れを数える code（T の T-RP-66-060〜063）は、probe の `native/hayamimi-asr/tests/r8v_probe.rs` に置き、`rp66.rs` には置かない。この遅れは probe の検出の時刻と manifest の `end_sample` から取る probe の量で、`rp66.rs` が E2E の run の event から 3.1 の式で取る `d` とは入力も式も違うからである。
R8v の最初の手番は、build も E2E の run も要らない offline の probe で、そこで止まる条件を 8 の R8v の行に置く。
高い E2E の run の前に、安い probe で見積りの前提を確かめるためである。
流れの途中の `flush` で detector が abort する形の再現と守りは RP-68 で、手番 2 の最初に通す。
残る穴は 10 の H68〜H79 である。
RP-67 の `L` を数える起点（`t_end`）の確かさの受入は RP-73 で、残る穴は 10 の H90〜H95 である。

### 3.18 訳の段の詰めの続き（R8w）

RP-32 と RP-33 を通すために、訳の段（`final` から `t_tr` まで）を縮める。
`docs/results/rp32/` の 3 対の記録では、RP-32 の対の比較が 19 発話中 14 で、下限の 17 に届かない。
RP-33 の訳の段は Rust の p50 285 ms、p95 1025 ms で、Python の 187 ms、890 ms を超える。
`L_asr` は 19 発話のすべてで Rust が速く、遅れは訳の段だけに在る（3 つとも原因の席の数えの転記）。
遅い発話は 4、8、12、22、23 で、3 対とも同じ発話が遅い（転記）。

#### 結論

作る物は 3 つで、この順に入れる。

1. **測り方の直し**：RP-32 の判定に訳の取れた割合を入れる（6 の RP-32 の行が持つ条件で、今の test が見ていない）。`/ingest` の port が移った run を無効にする（RP-72）。
2. **部分訳の使い回し**：`final` の文字列が、同じ発話の最後の速報の文字列と同じか、その末尾に `。` を 1 字足しただけの時、速報の T の答えをその行の T の答えとして使い、T を頼み直さない（RP-69、RP-70）。
3. **推測デコードの llama-server**：同じ model、同じ量子化（Q8_0）のまま、推測デコードを付けた llama-server を hayamimi 専用の unit で起こし、T と E の 1 回を縮める（RP-71）。

作らない物は 3 つである。
T と E0 を並べて投げる形（3.16 の 2 の A）、T の依頼の文の書き替え（発話 12）、量子化を軽くする形である。
理由は、この節の「採らない手段」に置く。

#### 数えた結果

数えた範囲は、2026-10-15 の main `3dc6402` の `native/` の `*.rs`（`target/` を除く）と `docs/results/rp32/` である。

| pattern（式） | 結果 | 陽性対照 | 扱い |
|---|---|---|---|
| `rs<N>.events.jsonl` の `final` ごとに、直前の `partial` の文字列と比べる `jq`（表の下） | 3 run とも 125 行中、そのまま同じが 0、末尾に `。` を足すと同じが 52、`？` を足すと同じが 9、句読点を除くと同じが 74 | 同じ file の `"type":"final"` の行が `rg -c` で 125 | 使い回す行を「同じ」と「`。` を 1 字足しただけ」に限る（今の記録で 52 行） |
| 同じ `jq` の結果を、`rs1.jsonl` の `finals` の数で `final` を発話へ順に結んで分けた物 | 遅い 5 発話のうち、4 と 23 は 5 回とも `。` を足しただけ、12 は 5 回中 1 回、8 と 22 は 0 回。8 と 13 の速報は ITN の前の文字列（`三十四件`、`九十一歳`）で、確定文は ITN の後（`34件`、`91歳`）。22 の確定文は速報より語が多い | 結んだ `final` の数 125 と、`final` の行の数 125 が同じ | 4 と 23 が対象に入る。8、12、22 は入らない。Python 以下に反転する見込みは 23 だけで、4 は T が V に落ちる行が 15/15 なので反転しない（10 の H80） |
| `rg -n 'retire\(\|\.note_line\(' --type rust native/hayamimi-relay` | 呼ぶ所は `partial.rs:299`（`Route::note_line`）の 1 か所。test は `partial.rs` の unit test 3 本と、`tests/r8d_partial.rs` の `the_final_of_the_utterance_drops_the_text_in_progress` | `partial.rs:161` の定義が当たる | 文字列の違う `final` が速報の依頼を止める形は変えない。r8d の test は速報と違う文字列（`japanese_text`）の `final` を送るので、そのまま通る。`partial.rs` の module の doc comment の「`final` は閉じた発話の速報を捨てる」は、実装の席が同じ commit で書き替える |
| `rg -n 'fn check_translated_share\|check_translated_share\(' native/hayamimi-eval` | 定義は `src/compare.rs:132`。呼ぶ所は `tests/support/e2e.rs:142` と、`tests/rp32.rs` の個別の test 2 本。RP-32 の判定 `check_alternated_runs`（`tests/support/rp32.rs:220-251`）は呼ばない | `compare.rs:473` の unit test が当たる | RP-32 の判定に入れる |
| `rg -n 'cache_prompt' --type rust native/hayamimi-relay/src native/hayamimi-eval/src` | 中継の依頼は `llm.rs:129` で真。`hayamimi-eval` の `translate_call.rs:28` は偽で、読む者は `stage_runs.rs` と `stage_load.rs`（3.5 の段の表と負荷）だけ | `translate_call.rs:92` の test が偽を固定している | 変えない。RP-32 と RP-33 の run は中継の依頼で測られている。RP-71 も中継の依頼で測る |
| 8080 の llama-server の起こし方（`systemctl --user` と `/proc/<pid>/cmdline`） | 別の app と共有の template の unit（`llama-artisan@.service`）で、その app が起こし止める。引数は `-m /home/fukugan/models/CAT-Translate-3.3b-Q8_0.gguf --host 127.0.0.1 --port 8080 -ngl 99 -c 16384` で、`-np` を持たない | 18081 の `hayamimi-llama-other.service` の引数が、その unit の file と同じ | 共有の unit は書き替えない。推測デコードは hayamimi 専用の unit で起こす |

句読点を除いて同じ 74 行と、`。` を足して同じ 52 行の差の 22 行は、`？` を足して同じ 9 行と、ITN か語が違う 13 行である。

```
jq -s -c '[.[] | .event | select(.type=="partial" or .type=="final") | {t:.type, x:.text}]
  | reduce .[] as $e ({last:null, maru:0, n:0};
      if $e.t=="partial" then .last=$e.x
      else (.n+=1 | (if .last!=null and (.last+"。")==$e.x then .maru+=1 else . end) | .last=null) end)
  | {maru, n}' docs/results/rp32/rs1.events.jsonl
```

#### 部分訳の使い回し（RP-69、RP-70）

中継は、速報の訳を確定文の T と同じ依頼の文（`translate_prompt`。`pipeline.rs:564` と `:618`）で頼む。
速報の job の `id` は、その発話が閉じる時の `final` の `id` と同じである（`partial.rs` の `Next::id`）。
したがって `final` の文字列が最後の速報と同じなら、速報の T の答えは、その行の T と同じ依頼への答えである。
今は `final` が来ると `Mailbox::retire`（`partial.rs:161-168`）が走っている速報の依頼を止め、lane が同じ文の T を頼み直す。
T の 1 回は約 223 ms（prompt 17 ms、19 token × 10.3 ms、HTTP 約 10 ms。転記）である。

決めは次のとおりである。

1. **結ぶ鍵**：音源、`id`、文字列の 3 つが合う時だけ使い回す。文字列は、`final` の文字列が速報の文字列と同じか、速報の文字列の後ろに `。` を 1 字足した物と同じ時に合う。`？` と `！` を足した形は合わない。問いか言い切りかが変わり、訳の文末と V の推量の読みが変わるからである。日本語が元の行（`final` の `lang` が `ja`）は、比べる前に両方の文字列から半角空白を取り除く。認識器は Latin の語と仮名の間の空白を、速報と `final` で付けたり付けなかったりし、空白 1 つの違いで T を頼み直す（約 300 ms）からである。英語が元の行は取り除かない。英語では空白が語の切れ目で、取り除くと別の語の並びが同じに見える。
2. **速報の答えを捨てない**：`final` が来た時、走っている速報の依頼の文字列が 1 の鍵で合えば、その依頼を止めずに、答えをその行に渡す。合わなければ今のとおり止める。待っている（worker がまだ取っていない）速報の文字列が合う時は、それを mailbox から外し、lane が自分で T を頼む。行に渡すと決めた依頼は、次の発話の速報が来ても止めない。
3. **lane の受け方**：lane は slot を取る前に mailbox に鍵を渡す。答えが在ればそれを受け、合う依頼が走っていればその答えを待ち、どちらも無ければ今のとおり slot を取って T を頼む。速報の依頼が失敗したか打ち切られた時は、待っていた lane が T を頼む。slot を持ったまま待たないのは、待たれている速報の依頼も slot を 1 本使っており、slot 4 本（3.6 の 5）の中で互いを待つ形になるからである。
4. **V は確定文を原文にして掛ける**：受けた T の答えは、`draft` の T の答えの後ろの処理（`pipeline.rs:635-650`。終わりの印の検査、observer、V）と同じ関数に通し、V の原文は `final` の文字列にする。速報で掛けた V の判定は使わない。`。` 1 字の違いでも、V が原文から読む物（数、否定、推量、名前、形）が変わりうるからである。V は 1 行につき多くて 2 回（T の答えと E の答え）のままである。V を通れば今と同じ先の訳（`of` が `final`、`stage` が `t`、`settled` が偽、`src_sha256` は `final` の文字列の sha256）を出し、落ちれば先の訳を出さずにすぐ E を頼む。
5. **log**：使い回した行ごとに `translate.draft.reused`（info）を 1 件出す。項目は `id`、`source`、`match`（`same` か `closing_mark`）、`state`（`done` か `running`）、`waited_ms`、`verdict`（`ok` か `held`）で、文字列は載せない。その行の `translate.line.ok` と `translate.line.held` の `t_ms` は、待った時間（`waited_ms` と同じ値）にする。
   T の訳が V に落ち、E の訳が通った行は、確定の前に `translate.draft.held`（info）を 1 件出す。項目は `id`、`source`、`hold_reason`（T の側の理由）で、文字列は載せない。`translate.line.ok` は T が落ちた理由を持たないので、T が落ちた行の数はこの event で数える。T が通った行と、E も落ちた行（`translate.line.held` が `t_hold_reason` を持つ）には出さない。
6. **変えない物**：速報の訳を出す条件と打ち切りの形（3.6 の 3）、wire の形、`final` の文字列が合わない時の挙動、E と V の依頼の形。

使い回しが効く機構は次のとおりである。
lat-ja では、最後の速報から確定文までに、`--min-silence` の無音と確定の復号が挟まる（ja の `d` の p50 881 ms、復号の p50 135 ms。3.17）。
T の 1 回（約 223 ms）は、その間に終わっているか、走っている見込みである（NOT VERIFIED。`translate.draft.reused` の `state` と `waited_ms` で数える）。
T が V に落ちる行も、T の 1 回を飛ばして E をすぐ頼めるので縮む。

`。` を足しただけの文の T の答えを使ってよいかは、訳の質の問いである。
原文の末尾の `。` の有無で、model の訳の文末の句点や語順が変わりうる。
RP-70 が、held-out の組で「末尾の `。` を除いた原文の訳」と「そのままの原文の訳」を同じ server で交互に打ち、前者の chrF と V が止めた行の数が後者より悪くないことを確かめる。
RP-70 が通らなければ、1 の鍵を「同じ」だけに狭める。
今の記録では「同じ」の行は 0 なので、そのとき使い回しは速さに効かない。

効きの見積り（NOT VERIFIED）は次のとおりである。
使い回した行の訳の段は、答えが出ていれば V と配信の数 ms、走っていれば残りの時間になる。
確定文の 42%（52/125）がそうなっても、残り 58% の行が今のままなら、全行の p50 はその 58% の下の側の値（今の V を通った行の p50 253 ms に近い値）に留まり、Python の 187 ms を下回るとは言えない。
p50 を下げきるのは、次の推測デコードと合わせた時である。
対の比較では、使い回しで Python 以下に反転する見込みの発話は 23 だけで、14 から 15 になる見込みである。
発話 4 も対象に入るが、T が V に落ちる行が 15/15 で、T を飛ばしても E の 1 回を待つので反転しない見込みである（評議の数えの転記）。
17 に届かない時の次の手は 2 つで、どちらも推測デコードの効きに頼らない（10 の H80）。
1 つ目は、3.17（R8v）で発話の終わりの待ち `d` を縮めることである。反転に足りない量は、発話 4 が約 240 ms、12 が 71 ms、22 が 202 ms である（転記）。
2 つ目は、3.16 の 2 の B のとおり、T が V に落ちた行の `hold_reason` を数え、正しい訳を止めている理由だけを直すことである。`docs/results/rp32/` には中継の stdout が無いので、次の RP-32 の Rust の run で 3.16 の 4 の手順どおり中継の stdout を run の file と同じ所に残す。

#### 推測デコードの llama-server（RP-71）

T の 1 回の大半は、出力の token の生成（19 token × 10.3 ms ≈ 196 ms。転記）である。
推測デコードは、安い下書き（ここでは小さい model）で数 token を先に出し、本体の model が 1 回の forward でまとめて照らす。
貪欲（`temperature` 0）に照らす時は、本体の model が 1 token ずつ選ぶ時と同じ出力の分布になる（S35）。
llama.cpp の推測デコードの文書も、出力を合わせるには貪欲の sampling を使うと書く（S34）。
したがって訳の質を動かさずに、1 token あたりの時間を縮められる見込みがある。
量子化は Q8_0 のまま変えない（8 の R8u の決め）。

1 回の時間の見込みは、次の式で置く（S35 の式）。
`α` は下書きの token が採られる率、`γ` は 1 回に下書く token の数、`c` は下書きの 1 token の時間と本体の 1 token の時間の比、`t_tok` は本体の 1 token の時間である。

```
1 回の照らしで進む token の数の期待値 = (1 − α^(γ+1)) / (1 − α)
1 token あたりの時間 ≈ t_tok × (1 + γ c) / ((1 − α^(γ+1)) / (1 − α))
```

`α` は、この model の訳の依頼では測っていない（NOT VERIFIED）。
例として `α` 0.7、`γ` 3、`c` 0.25 なら、1 token あたりの時間は 0.69 倍で、T の生成は約 196 ms から約 135 ms になる。
model の要らない n-gram の下書きは腕に入れない。
llama.cpp の既定の値では、`ngram-simple` は 12 token、`ngram-mod` は 24 token の並びで照らし、`ngram-mod` は 48 token 以上を下書く（S33、S34）。
T と E の答えは 19〜45 token なので、既定の値では下書きがほとんど出ない。
値を小さくする形と E にだけ掛ける形は、どの値が効くかの根拠が無く、T と E は同じ server に頼むので E だけに掛ける口も確かめていない。

決めは次のとおりである。

1. **起こし方**：共有の 8080 の unit は書き替えない。hayamimi 専用の user unit `hayamimi-llama-pair.service` を作る。unit の file は `native/hayamimi-relay/units/` に置き、`hayamimi-llama-other.service` と同じ形で `~/.config/systemd/user/` へ写す。引数は `-m /mnt/h3-models/ollama/CAT-Translate-3.3b-Q8_0.gguf --host 127.0.0.1 --port 18083 -ngl 99 -c 8192 -np 4` に、2 の腕の引数を足した物である。中継は `--llm http://127.0.0.1:18083` で向ける。Windows の `hayamimi llama start`（`native/hayamimi/src/llama/`）に同じ腕の引数を足すのは、RP-71 が通った後の手番である。
2. **腕**：RP-71 を流す前に、次の 2 つに固定する。
   - **N0**：推測デコードなし。
   - **S3**：`--spec-type draft-simple --spec-draft-model <下書きの GGUF> --spec-draft-n-max 3`（`--spec-draft-n-max` は既定の値）。下書きの model は、公開の GGUF `mmnga-o/CAT-Translate-0.8b-gguf` の `CAT-Translate-0.8b-Q8_0.gguf`（845,363,136 bytes、Hugging Face の LFS の sha256 `f652c0cd4352424a744bae9e9f633c2efd16a99c0f1a48a0e00d171dc9d6949f`。S37）を使い、変換はしない（評議の判定）。`CAT-Translate-0.8b` の基は `sarashina2.2-0.5b`、許諾は MIT である（S36）。取る先は `/mnt/h3-models` の下で、取った file の sha256 を `docs/results/r8w/server-args.md` に記録する（記録で、起こす条件にしない）。
3. **S3 を流す前の確かめ**：下書きの model と本体の model が同じ語彙を持つことを、次の 2 つで確かめる。1 つ目は、Hugging Face の `cyberagent/CAT-Translate-0.8b` と `cyberagent/CAT-Translate-3.3b` の `tokenizer.model` の sha256 が同じであること（評議の数えでは、どちらも `008293028e1a…` で、`tokenizer.json` の違いは `truncation.max_length` の 700 と 2048 だけ、`model.vocab` は 102400 件、`added_tokens` も同じ。転記）。2 つ目は、腕 S3 で起こした llama-server の起動の log に、下書きの model の語彙が合わないという行が無く、server が `/health` に `ok` を返すことである。どちらかが外れたら S3 を流さず、推測デコードを採らない。
4. **選び方**：S3 が RP-71 の期待値を全部通れば S3 を採り、通らなければ推測デコードを採らない。選び方は、結果を見てから動かさない。
5. **測る時の起こし方**：GPU の memory は、評議の数えの時点で 16311 MiB 中 12400 MiB が使われ、8080 の 3.3b、18081 の TranslateGemma、ComfyUI が載っている（転記）。3.3b の Q8_0 は file だけで 3,568,393,024 bytes（約 3403 MiB）、下書きの model は約 806 MiB で、そのまま足すと RP-22 の上限 14,000 MiB を超える。したがって起こす前に次の順で打つ。
   1. `systemctl --user stop hayamimi-llama-other.service` で 18081 の server を止める（RP-71 の run は ja と en の間だけを訳すので使わない。止めても中継は止まらず、組の外の言語の行が `held` になるだけである。3.6）。
   2. `nvidia-smi --query-gpu=memory.used,memory.total --format=csv,noheader,nounits` を 1 回読み、値を `docs/results/r8w/server-args.md` に書く。
   3. 読んだ `memory.used` に、本体の file の大きさ（約 3403 MiB）と、S3 では下書きの file の大きさ（約 806 MiB）を足した値が 14,000 MiB 以下の時だけ、`hayamimi-llama-pair.service` を起こす。超える時は起こさず、読んだ値を添えて評議へ出す（8080 の共有の server と ComfyUI は別の持ち主の物で、この run の側から止めない）。
   4. 起こした後に `/health` が `ok` を返してから同じ `nvidia-smi` を読み、`memory.used` が 14,000 MiB 以下であることを記録する。run の間も RP-22 と同じく 1 秒ごとに読み、14,000 MiB を超えたら run を止めて評議へ出す。
   5. llama-server は `sudo -n renice -n -5 -p <pid>` で上げ、測る client は `sudo -n nice -n -5 <command>` の形で起こす。測るのは E2E の席で、この節を書いた席は測らない。
   6. run の後に `systemctl --user stop hayamimi-llama-pair.service` と `systemctl --user start hayamimi-llama-other.service` を打ち、18081 の `/health` が `ok` を返すことを確かめる。
6. **戻し方**：起こす前に、`docs/results/r8w/server-args.md` に、共有の unit の `ExecStart` の行（上の表の引数）と、新しい unit の腕ごとの `ExecStart` の行を書いておく。戻す時は `systemctl --user stop hayamimi-llama-pair.service` を打ち、中継を `--llm` を付けずに（既定の 8080 で）起こし直す。unit の file の頭の comment に、起こす、確かめる、止める、外す command を書く（`hayamimi-llama-other.service` と同じ形）。共有の unit は触らないので、そちらに戻す物は無い。
7. **llama.cpp の版**：推測デコードの引数の名前は版の間で変わっている（`--draft` と `--draft-min` は外され、`--spec-draft-n-max` に替わった。S33）。WSL の build（`build-cuda120`）と Windows の b11609 が `--spec-type` を持つかは確かめていない（NOT VERIFIED）。流す前に、llama.cpp を使う前に fetch して build し直す規則（hooks の `guard_commands.rs`）のとおり最新に上げ、`llama-server --help` の出力に `--spec-type` と `--spec-draft-model` が在ることを `docs/results/r8w/server-args.md` に記録する。

#### 測り方の直し（RP-32 の判定、RP-72）

RP-32 の判定 `check_alternated_runs`（`native/hayamimi-eval/tests/support/rp32.rs:220-251`）は、6 の RP-32 の行が持つ「Rust の有効な標本のうち訳の取れた割合が 20 / 23 以上」を見ていない。
`compare.rs:132` の `check_translated_share` を Rust の対を合わせた標本に掛け、`judge_pooled` の失敗の列に足す。
受入の定義は 6 の RP-32 の行のままで、新しい ID を置かない。
同じ条件を 2 か所に書くと、片方だけが直されるからである。
今の 3 対の記録で割合が足りているかは数えていない（NOT VERIFIED。直した test を今の dir に打てば出る）。

`hayamimi` は、port が使えなければ次の 10 個から空いた port へ移って動き続ける（3.15 の port の行、`server.rs:168-182` の `bind_on`）。
止めない形は変えない。
ただし前の run の server を止めた直後に次の server を起こすと、`/ingest` の 8766 が取れずに移ることがあり（10 の H83）、測りの道具は 8766 へ送り続ける（`hayamimi-eval` の `--port` の既定）。
そこで次の 2 つを足す。

1. **手順**：各 run の前に、前の run の process を止めた後、8766 と 8833 に待ち受けも接続も残っていないこと（WSL は `ss -tanH 'sport = :8766 or sport = :8833'` の出力が 0 行、Windows は `Get-NetTCPConnection -LocalPort 8766,8833 -ErrorAction SilentlyContinue` の出力が空）を 1 秒ごとに確かめ、空いてから次の server を起こす。待った秒数は、run の dir の `ports<N>.txt` に run ごとに 1 行で書く（記録で、判定には使わない）。この手順は `docs/results/e2e/README.md` の「Conditions of the run」に足す。
2. **無効にする**：RP-32 の dir の `rs<N>.events.jsonl` に、`type` が `fallback` で `reason` が `in_use` の event が 1 件でも在る run は無効とし、RP-32 の判定はその run の名前を挙げて値を出さずに落ちる。`rs<N>.events.jsonl` が無い run も、確かめられないので落ちる（RP-72）。

#### 構造

```
final（音源、id、文字列）
   │
   ├─ mailbox に鍵を渡す ─┬─ 合う答えが在る ─────────────────┐
   │                      ├─ 合う依頼が走っている ── 待つ ────┤
   │                      └─ 無い ── slot を取って T を頼む ──┤
   │                                                          ▼
   │                                  V（原文は final の文字列）
   │                                    ├─ 通る ─▶ 先の訳 ─▶ E ─▶ V ─▶ 確定の訳
   │                                    └─ 落ちる ─────────▶ E ─▶ V ─▶ 確定の訳
   │
   └─ T と E の依頼の行き先は、RP-71 で採った腕の llama-server（hayamimi-llama-pair、18083）
```

図の結論：使い回しは T の 1 回だけを飛ばし、V と E と確定の訳の決め方は今のままである。

#### 採らない手段

- **T と E0 を並べて投げる（3.16 の 2 の A）**：この節では作らない。H66 の probe で、E0 の 1 回の p50 は 640 ms（76 本）で、今の T が V に落ちた行の訳の段の p50 518 ms（転記）より長い。A の落ちた行の最初の訳は E0 の答えを待つので、今より遅くなる見込みである。2 回打って同じ出力の行も、E0 は 38 行中 24 で、今の T の 34 より少ない。使い回しが入ると、対象の行は T を飛ばして E をすぐ頼めるので、A の効く行はさらに減る。A は 3.16 の 2 の作る条件から外す（評議の判定）。作るかを開き直すのは、E0 の 1 回の p50 が E の 1 回の p50 を下回った時だけである。
- **T の依頼の文を書き替える（発話 12 の、訳に日本語が残る形）**：T の依頼の文は model card の形に固定し、`curl` と同じ body で打った訳と一致することを受入にしている（3.6 の「訳の依頼の形」）。書き替えると全行の T が変わり、RP-13 と RP-35 を打ち直すことになる。H66 の probe でも、T に 1 文を足した腕 A は、新しい数と途中で切れた訳を出した。発話 12 の差は 69 ms（転記）である。
- **量子化を軽くする**：Q8_0 のまま据え置く（8 の R8u）。軽くすると出力が変わり、推測デコードと違って質が同じとは言えない。
- **共有の 8080 の unit に推測デコードの引数を足す**：その unit は別の app が起こし止め、同じ port で別の model も読む。引数を足すとその app の速さと memory が変わり、hayamimi の run の側から戻せない。
- **port が移ったら起動を止める**：3.15 の止めない形に反する。測る run の側で空くのを待ち、移った run を無効にすれば足りる。
- **句読点を全部除いて比べて使い回す**：`？` と `。` の違いは文の型の違いで、訳の文末と V の推量の読みを変える。
- **速報に ITN と句読点を掛けて、使い回す行を広げる**：発話 8 と 13 は対象に入るが、`hayamimi-asr` の速報の経路（R8v の席が触る `pipeline.rs`）と、画面に出る速報の文字列が変わり、RP-5 と RP-25 を打ち直すことになる。広げない（評議の判定）。
- **速報の T が V に落ちた時に、E を速報の段で先に頼む**：速報の文字列が変わるたびに E の 1 回と slot を使う。確定文と同じ文字列かは `final` が来るまで分からない。
- **測りの道具の `cache_prompt` を真にする**：`translate_call.rs` は 3.5 の段の表のための物で、同じ文を繰り返し打っても計算し直す形を測っている。RP-32 と RP-33 は中継の依頼（`cache_prompt` は真）で測られている。

#### 順と置き場

1. RP-32 の判定の直しと RP-72（`native/hayamimi-eval/tests/support/rp32.rs`、`tests/rp72.rs`、`docs/results/e2e/README.md` の手順）。次の RP-32 の Rust の run の前に入れる。製品の code は変えない。
2. 部分訳の使い回し（`native/hayamimi-relay/src/partial.rs` の `Mailbox` と `Route::note_line`、`relay.rs` の `draft_line`、`pipeline.rs` の `stage_to` に T の答えを外から受ける入口）。RP-69 を通す。並べて、E2E の席が RP-70 を流す。
3. 推測デコードの probe（RP-71）。2 と触る物が重ならないので、別の席で並べて進めてよい。S3 が通ったら、unit の file と `hayamimi llama start` の引数を同じ手番で入れる。
4. 2 と 3 が入った build で、RP-32 の Rust の run と、RP-33、RP-25、RP-35 を打ち直す（E2E の席。3.16 の 4 の手順と、この節の port の手順）。2 と 3 の効き目を分けて読むために、2 だけの build の 1 本を先に流してよい（記録だけで、判定に使わない）。

受入は 6 の RP-69〜RP-72 と、既存の RP-32、RP-33、RP-25、RP-35 で、test の名前は 7 の表、slice は 8 の R8w、残る穴は 10 の H80〜H85 である。
RP-32 の判定の直しの陰性の case（訳の取れた割合が 20 / 23 未満で、`L` が Python 以下の Rust の対）は、test-designer の席が RP-32 の case 行に足す。

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
- **load の記録の無い Python の記録と比べる**：負荷が揃っているかを読めず、差が実装の差か機械の状態の差かを分けられない（3.16 の 4）。RP-32 の基準の 3 run は、各 run の前後の load の記録を持つので、この形に当たらない。
- **RP-32 のたびに Python を Rust と交互に流し直す**：Python の run は置き換える前の物の基準を取るための物で（8 の R0）、基準は load の記録つきの 3 run として `docs/results/rp32/` に在る。流し直すと、R11 で消す実行時の Python を測りのたびに動かし続ける。負荷の差の向きは、Rust の run の前の load を基準の load と並べて読む（3.16 の 4）。
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
| RP-1 | 認識の誤り率（scorecard と同じ組と採点） | `<ev> scorecard --engine hayamimi --sets testdata/eval_real,testdata/eval_real_zhko,testdata/eval_real_yue --out rp01.json` の後に `cargo test --release --test rp01 -- --ignored` | 言語ごとの平均の誤り率が、R0 の 2 回の Python の出力（`<base>/scorecard_run1.md` と `scorecard_run2.md`、RB-3）の言語ごとの平均以下（ja、zh、ko、yue は CER、en は WER）。Python の値は小数 3 桁に丸めた記録しか無いので、Rust の平均も小数 3 桁に丸めて（四捨五入）から比べる。測るのは配る形の build（自分で build した sherpa-onnx と 3.12 の ORT）で、Python の wheel の ORT を読ませた run は、Rust の code が Python と同じ物を出すことの確かめ（clip ごとの値が Python の log と小数 3 桁で一致）にだけ使い、合否に使わない。言語判定は `hayamimi-asr` の振り分けで決め（clip の正解の言語を渡さない）、LID の正解の数を数える。yue は Python と同じく OpenCC の t2s で正規化してから採点し、Rust の側は `ferrous-opencc`（0.4.0、Apache-2.0、OpenCC の Rust 実装）の t2s を使う（10 の H24）。LID の正解の数も Python 以上。値は `docs/results/scorecard.md`（ja 0.038、en 0.023、zh 0.066、ko 0.081、yue 0.061）と並べて記録する。Windows の配る形の build で振り分けを通した run では、zh と yue は Python と同じ値である（10 の H35。R3c の検証の席の可、`b8fd07e`） | `<base>` の Python の値を同じ test が読み、scorecard.md と 0.005 以内で一致する | 振り分けで ja の clip を SenseVoice の tier へ送る build では、ja の CER と ja の LID の正解の数が Python より悪くなり落ちる（scorecard の経路は句読点の段を通らず、採点も句読点を捨てるので、句読点を外した build は通常の run と同じになり、対照にならない） |
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
| RP-13 | 訳の品質 | `<ev> fleurs-pairs --targets en,zh,ko,es --n 50 --seed 0 --out pairs/` の後に `hayamimi-relay eval --pairs pairs/ --out hyps/`（中継の `eval` は slice 1 に無く、R8f で足す。llama-server が要る）、`<ev> chrf --pairs pairs/ --hyps hyps/` | 確定の訳の corpus chrF が en 47.57、zh 20.19、ko 25.20、es 42.09 を超える（en は会話字幕の CS-3 と同じ）。V が両方の訳を止めた行は空の hyp で数える。RP-13 は未通過のまま、基準値 47.57／20.19／25.20／42.09 は動かさない。test split の V ありは en 44.40・zh 19.55・es 33.56 が記録未満で ko 26.18 のみ超え、V なしは 4 言語とも超えるが記録は seed 0 dev の組なので参考値。R11 入口の RP-13 は、V なしの chrF を記録と同じ組で測って 4 言語とも超えることで決め、V ありの値は止め率つきの報告に留める（評議の判定。test split の V なしは en 55.28、zh 26.46、ko 29.99、es 46.13。10 の H36）。RP-13 の入口の条件は満たした（dev の seed 0、2026-10-15 05:1x、1 回、検証の席の申告）：V なし（T だけ）は en 54.59、zh 28.78、ko 27.76、es 45.79 で 4 言語とも記録を超える。V あり（T だけ、止めた行は空）は en 48.33、zh 24.07、ko 24.11、es 39.70 で、止めた行は 6、7、8、8（各 50 行）、ko と es は記録を割る。eval（T、E、V）は en 49.04、zh 24.07、ko 24.11、es 39.68 で、`translate.eval.held` は 28 件。同じ組で打ち直した再現は無く、T の訳が llama.cpp や model の版で変わらないかは確かめていない（NOT VERIFIED） | 参照訳を hyp に渡すと 100 | 原文を返した hyp では値を割る |
| RP-14 | Rust の chrF が sacrebleu と同じ値を出す | R0 で sacrebleu 2.6.0 が採点した組（`<base>/chrf_fixture.tsv` と値）を `cargo test --release --test rp14` が読む | 差の絶対値が 0.01 以下 | 同じ fixture の文 1 本 | n-gram の上限を 5 にした実装で落ちる |
| RP-15 | 話者の分離 | `<ev> der --set testdata/eval_diar --collar 0.25` の後に `cargo test --release --test rp15 -- --ignored`。確定文が話者を待たないこと（T-RP-15-012、013）は `cargo test --release -p hayamimi --test rp15_speaker -- --ignored` | AMI 5 会議の平均 DER から `<base>` の Python の 3 回以上の run の平均を引いた差が、Python の回の間の幅（平均 DER の最大 − 最小）より小さい（Python の平均以下なら差は 0 以下で通る。差が幅と等しいか大きければ落ちる）。差と幅を同じ結果の file に書く。幅で読むのは、Python の pyannote と FastClustering の run ごとの揺れの中では、Rust の値が Python より悪いとも良いとも言えないからである | `<base>` に Python の `--method refine_diarize` の run が 3 回以上在り、同じ test がその平均と幅を出せる（2 回では幅を決めない）。`diarization.md` の値は入力の音声の sha256 が残っていない記録なので比べない | 清書の再分離を切った run では落ちる |
| RP-16 | GPU の無い機械では段ごとに CPU へ落として字幕を出す | `CUDA_VISIBLE_DEVICES` を空にして `<hy> --wav tests/golden/ja/ja_014.wav --no-realtime --mode single --lang ja` を起こし、同じ run の `/events` を読む `cargo test --release --test rp16 -- --ignored` | 終了 code が 0、`final` が 1 件以上、`gpu.device.unavailable` が 1 件（`reason` が `no_device`）。その run が読み込む 3.5 の GPU の段のそれぞれに `gpu.fallback.cpu` が 1 件ずつ（R2 の ja の run では句読点の 1 件、R3 以降は言語判定も）、同じ段の `/events` の `fallback`（`to` が `cpu`）が 1 件ずつ。`gpu.session.created` の実際の provider が全件 `cpu`。stderr に `nvidia-smi` の名前が在り、`LANG=ja_JP.UTF-8` と `LANG=C.UTF-8` で stderr の文が別の言語 | GPU の在る run では `gpu.device.unavailable` と `gpu.fallback.cpu` と `fallback` の event が 0 件で、GPU の段の provider が `cuda`（GPU が在るのに CPU へ落ちない） | GPU が在るのに CPU へ落ちる build（session を `cpu` で作る build）では、GPU の在る run で `gpu.fallback.cpu` か `cpu` の provider が出て落ちる。落ちたのに event を出さない build では、GPU を隠した run で event の数が足りず落ちる |
| RP-17 | 自分で build した library が開け、ORT が 1 つである（R1 の門） | Windows と WSL で `hayamimi-gpu-probe`（`hayamimi-gpu` crate の bin。`native/target/<triple>/release/` に在る）を走らせ、`cargo test --release --test rp17` が子の process の出力を読む（`rp17.rs` の test は `#[ignore]` を持たないので `--ignored` を付けない。付けると 0 本しか走らない）。Linux 限定の test（T-RP-17-031）は WSL から打つ。`hayamimi` crate は R2 で作るので、`<hy> gpu-probe` はそれまで無い | `cuda_devices: 1`、sherpa-onnx の CUDA の session が 1 本作れて実際の provider が `cuda`、`ort` が開いた ORT の `GetVersionString` が `build.json` の ORT の版（1.31.0。退路へ戻したときは 1.28.3）と同じ、process の中の `onnxruntime` の library が 1 つで、その読み込み元が実行 file と同じ directory（probe の `modules` の origin が `exe_directory`）。CUDA の版は、認識器が開いている間の process の読込 module の名前で見る（`build.json` の申告ではない。名前は大文字と小文字を区別せずに数える）。Windows は `cublas64_13` と `cublasLt64_13` が各 1、`cudart64_12` と `cublas64_12` と `cublasLt64_12` が 0。`cufft64_12` は在ってよい（CUDA 13.4 の toolkit 自身が `cufft64_12.dll` を持つので、`*_12` を一律 0 にはしない）。WSL は `libcudart.so.12` が 0、`libcublas.so.13` が 1。ORT の CUDA の provider の library の中の文字列では CUDA の版を判別しない（`cudart64_12.dll` と `cudart64_13.dll` の文字列を両方持つ）。Windows では、`hayamimi-gpu` を依存に持つ全 crate の bins と tests に 3.4 の 2 本の link 引数が在る（`cargo metadata` で依存する crate を数え、各 crate の build script を読む）。通ったときは、その build の `build.json` の sha256 と probe の出力を `native/target/sherpa-gpu/<os>/rp17_passed.json` に書く（RP-24 が読む） | 2 つの OS で同じ出力の形 | 配布の CPU 版の sherpa-onnx に繋いだ build では provider が `cpu` で落ちる。`onnxruntime` の読み込み元が `system` の run で落ちる |
| RP-18 | 句読点の同一性 | `cargo test --release --test rp18` が `mobile/hayamimi_core/test/fixtures/punct_ja_parity.json` を読む | 全 case で `input_ids` が一致。GPU で `expected` の文字列が全件一致 | fixture の 1 件目 | NFKC を外した token 化で落ちる |
| RP-19 | Python の test の case を Rust へ写したこと | `cargo test --release -- --list` の出力と、test-designer が `rust_port_tests.ja.md` に書く対応表を `cargo test --release --test rp19` が突き合わせる | 対応表の全 386 行が、Rust の test の名前か `写さない：<理由>` のどちらかを持つ。名前を持つ行は list に在り、全部通る。名前を持つ行のうち list に無い行は `native/hayamimi-eval/tests/rp19_not_yet.txt` の名前つき一覧に在り、一覧の行の数は `rp19.rs` の `ALLOWED_NOT_YET` 以下で、`ALLOWED_NOT_YET` は下げる向きにだけ動く。一覧が空でない間、`rp19_python_cases_are_ported` は一覧の数を出して落ちる（8 の「RP-19 の未実装 187 行の仕分けと slice」）。`写さない` を許すのは 5 種だけである。5 つ目は Rust が意図して持たない Python の口で、`写さない：Python の口（<Rust の在り処、または持たない機構>）` と書き、在り処も機構も無い行は許さない（同じ小節の「写さない行」の 15 行）。ほかの 4 種は、3.2 が消す script の test、この設計で無くなる部品（Python の中の翻訳 worker と翻訳器の pool、文ごとの訳の分割。訳は `hayamimi-relay` が 1 行ずつ頼む、3.6。翻訳 worker が hub に訳を出す case は中継へも写さず、中継が訳を出すことは会話字幕の CS-21 と CS-22 が持つ）、外が更新する model の sha256 を動作の gate にする case（sha256 は `models/manifest.json` の記録に留める、3.10）、Python の実行時の package の import に固有の挙動（`soundcard` の import の再試行。Rust には実行時の import が無く、装置が開けない時の落とし方は RP-29 の R5 の行が持つ）である。`docs/spec/ja_pipeline_spec.json` の `config` を `<hy> config dump` で作り直した値と差 0 | 対応表の 1 行目 | 名前を 1 つ変えた list で落ちる |
| RP-20 | 3.5 の表の全段の置き場が決まったこと | `cargo test --release --test rp20` が 3.5 の表の「いまの置き場」の列に「測る」が残っていないことと、各行が `gpu_stages.md` の行に対応することを確かめる（test は `#[ignore]` を持たない。docs だけを読む） | 「測る」が 0 件、表の行と結果の行の対応が全件 | R1 の前の表（`native/hayamimi-eval/tests/fixtures/rp20_table_before_r1.md`、「いまの置き場」の列に「測る」が 8 件）を陽性対照の入力にし、同じ式が 8 件と数える | 結果の file の行を 1 つ消すと落ちる |
| RP-21 | model の取得に Python が要らない | 空の directory で `<hy> models fetch --minimal --dir <tmp>` | 今の `--minimal` と同じ 4 組（ReazonSpeech、whisper-tiny、Silero、句読点）が 3.5 の置き場に合った版（3.10）で揃い、`models/manifest.json` に sha256 が在る | 既に在る directory で打つと取り直さない | 取得先の 1 つを壊すと、もう 1 つの取得先（sherpa-onnx の release と Hugging Face）から取る。両方を壊すと前の file を残し、`models.fallback.previous` を 1 件出して終了 code 0 |
| RP-22 | GPU の memory の上限 | RP-3 の run の間、`nvidia-smi --query-gpu=memory.used --format=csv` を 1 秒ごとに読む（R3b は `/ingest` が入る前なので、llama-server を起こした機械で `<hy> --wav testdata/lat-multi.wav` を実時間で流す間に読む。8 の「07:00 の計画」の R3 の着手の 2） | 最大が 14,000 MiB 以下（`gpu_stages.md` に memory の実測が無いまま据え置いた値で、R3 と R8e が測り、超えたら評議へ戻す。16,311 MiB から、他の process の 1,161 MiB と、余白の約 1,150 MiB を除いた値） | 起動前の値が読める | 上限を、その run の起動前の読みの最大より 1 MiB 高い値に置いた test の設定（読む物は GPU 全体で、他の process の使用量は run ごとに動くので、別の run の最大から置いた上限は、その run が届かない所に在りうる。起動前の読みから置いた上限は、読み込んだ後の `hayamimi` の分だけ必ず下回る）では、`hayamimi` が止まらず `gpu.fallback.cpu`（`memory_budget`）を 1 件以上出し、test の比べる式は上限を超えた読みを名指しして落ちる（比べる式が働くことの確かめ） |
| RP-23 | 段ごとの GPU と CPU の計測（R1） | `<ev> stages --provider cuda --wav lat-ja.wav --out stages_cuda.jsonl` と `--provider cpu` を同じ build で 1 回ずつ（1 stream）打ち、同じ 2 本を llama-server に訳の依頼を流し続けた負荷の下でも打ち、`cargo test --release --test rp23 -- --ignored` が `docs/results/gpu_stages.md` を書いて読む | 3.5 の表で GPU の経路が在る段（「いまの置き場」が「CPU（GPU 経路なし）」で始まらない 14 段）について、無負荷と負荷の下の、GPU と CPU の p50 と p95（標本は lat-ja の 23 発話を 5 回流した区切りの数か、文字列の段は同じ数の行）、1 発話あたりの転送の時間と bytes、測った機械と `build.json` の要点が在る。JIT の cache が空の run と温かい run の `decode_ms` が別の行で在る（10 の H27） | 同じ build で同じ入力を 2 回流した p50 の差が 10% 以内 | provider を指定しない run の行は、CPU の行と同じ値になり、GPU の行として受け付けない |
| RP-74 | encoder だけを GPU に置いた形の認識の時間の見積り（3.5 の「認識の encoder だけを GPU に置く案」。置き場を変える前の門） | Windows で、3 条件（無負荷、`--load-url <llama> --load-clients 2`、`--peers 1`）ごとに、`<ev> stages --provider cpu --wav lat-ja.wav --models models --repeat 5 --cache warm --only recognize,encoder_only,search_replay --run <k> --out docs/results/rp74/<条件>_cpu_<k>.jsonl` と `<ev> stages --provider cuda --wav lat-ja.wav --models models --repeat 5 --cache warm --only encoder_only --run <k> --out docs/results/rp74/<条件>_cuda_<k>.jsonl` を k = 1、2、3 で別の process で交互に打つ（k が奇数なら cpu が先）。窓の前に `hayamimi-eval.exe stages --help` が `--only`、`--peers`、`--run`、`--cache` を持つことを確かめ、全 run を peer を含めて同じ高い優先度（WSL は `sudo -n nice -n -5`、Windows は `cmd.exe /c start "" /high /wait`）で起こす。各 run の直前と直後の WSL の `/proc/loadavg`、Windows の `Get-Counter '\Processor(_Total)\% Processor Time'`、`pgrep -x rustc`、`pgrep -x cargo`、`Get-Process -Name rustc,cargo`、`nvidia-smi --query-compute-apps` を `docs/results/rp74/load.txt` に、各 process の `PriorityClass` を `priority.txt` に書き、`cargo test --release --test rp74 -- --ignored` が読む（3.5 の「確かめる probe」） | 条件ごとに、組 k の B の標本を同じ `(utt, repeat)` の fbank（int8）+ `encoder_only`（cuda）+ 0.5 ms + `search_replay` の joiner と decoder（int8、cpu）の和とし、115 個の和から p50 と p95 を取る（p95 どうしを足さない）。A は `recognize_total`（int8、cpu）。3 条件の全部で、3 組の B の p50 の中央値が A の p50 の中央値より「15 ms と、3 組の A の p50 の幅の大きい方」以上小さく、B の p95 の中央値が「A の p95 の中央値 + 3 組の A の p95 の幅」以下。GPU の負荷の下の条件だけが落ちた時は、test は「評議へ戻す」と言って落ち、A へ戻したとは書かない。各 run の直前の WSL の load が 2 未満、Windows の CPU が 20% 未満、直前と直後の rustc と cargo が 0 件、llama-server と run 自身の他に compute app が 0 件、全 process が `High`（外れた run は捨て、名前と居た process を挙げる。捨てた後に組が 3 つ揃わなければ落ちる）。3 条件の数と差を `docs/results/rp74/summary.md` に書く | 足し算の式の確かめ：同じ CPU の run の fp32 で、fbank + `encoder_only`（cpu）+ `search_replay` の和が `recognize_total`（fp32）の p50 の ±15% に入る。入らない条件が 1 つでも在れば、見積りを使わずに「式が成り立たない」と言って落ちる | `encoder_only` の p50 を同じ組の A_k に置き換えた fixture では、見積りが A を超えて落ちる |
| RP-75 | split の形の認識の結果と、同じ model の file を CPU で読んだ sherpa-onnx の結果の一致（3.5 の「B を採るときの front end と探索」） | `cargo test --release --test rp75 -- --ignored`。test は `tests/golden/ja/` の 8 本と `testdata/eval_real` の ja の 15 本を、参照（sherpa-onnx、`provider` `cpu`、encoder は fp32、decoder と joiner は int8、`modified_beam_search`）と split の形（`--asr-encoder cuda` の形の認識器）で 1 回ずつ解く | 23 本のうち 22 本以上で token の列が一致し、一致しない clip は 2 つの text と CER を並べて書く。正解の text に対する 23 本の CER（`cer_dist` の和 ÷ `cer_denom` の和）の差が 0.002 以下。B を採る時は前段として、B の encoder を CPU にした run が参照と 23 本の token の列と時刻で全部一致し、fbank が dump の道具の参照と全 frame の全 bin で `f32::to_bits` が一致する | 参照を 2 回解いて 23 本が全部一致する（参照が決まった値を出す） | 参照の text を別の clip の順に並べ替えた fixture では一致が 22 本に届かず落ちる |
| RP-76 | split の形の ja の誤り率が、今の形（全部 CPU の int8）より悪くならない | RP-1 と同じ形の `<ev> scorecard --engine hayamimi --asr-encoder <形> --sets testdata/eval_real --out rp76_<形>.json` を、同じ build で `<形>` を `cuda` と `cpu` にして 1 回ずつ打ち、RP-2 の golden も両方で解き、`cargo test --release --test rp76 -- --ignored` が読む | ja の CER（小数 3 桁に四捨五入）が、split の形で今の形以下。RP-2 の golden の CER も今の形以下。en、zh、ko、yue の tier は split の形でも今と同じ値（Rz の tier だけを変えるので） | `--asr-encoder cpu` の run の ja の CER が `scorecard.md` の 0.038 と 0.005 以内で一致する | ja の clip を SenseVoice の tier へ送る build では ja の CER が悪くなり落ちる |
| RP-77 | GPU が使えない時に、認識器が今の形へ落ち、落ちたと出す（3.5 の「GPU が無いときの経路」、3.11） | `CUDA_VISIBLE_DEVICES` を空にした `<hy> --asr-encoder cuda --wav lat-ja.wav` を 1 回と、GPU の在る同じ command を 1 回流す。後段として、`nsys profile --stats=true <ev> stages --provider cuda --asr-encoder cuda --wav lat-ja.wav --models models --repeat 5 --only recognize --profile-ranges --out docs/results/rp77/split.jsonl` と、同じ build の `--only encoder_only --profile-ranges` を 1 回ずつ打つ（Windows の profiler で取れない時は WSL の同じ build）。`cargo test --release --test rp77 -- --ignored` が stdout の log、`/events` の記録、2 つの profile の統計を読む | 落ちた run で、`gpu.fallback.cpu`（`stage` は `recognizer`、`reason` は `no_device`）が 1 件、`gpu.session.created` の `encoder-epoch-35-avg-1.int8.onnx` の実際の provider が `cpu`、`/events` に `{"type":"fallback","stage":"recognizer","to":"cpu"}` が 1 件在り、`final` の text が `--asr-encoder cpu` の run と全部同じで、終了 code が 0。後段で、split の認識器の 1 発話の host → device の回数が、同じ発話の `encoder_only` の回数 + 4 以下（decoder と joiner が CUDA に居れば frame ごとに往復が足され、lat-ja の p50 の 146 frame で 290 回以上増える）。patch の効き目はこの観測で決め、`build.json` の patch の sha256 は記録として並べるだけにする | GPU の在る run では、`encoder-epoch-35-avg-1.onnx` の実際の provider が `cuda` で、`stage` が `recognizer` の fallback が 0 件 | GPU の在る run を落ちた run として渡すと、fallback が 0 件なので落ちた run として受け付けない。落ちた run の log から `gpu.fallback.cpu` を消した fixture は、黙って落ちたとして落ちる。patch を当てない library に繋いだ split の run は、host → device の回数が今の CUDA の形（p50 で 450 回）に近づき、後段で落ちる |
| RP-78 | split の形で `L_asr` が縮み、訳が遅くならない（E2E） | RP-32 の規則（同じ機械、run ごとに `hayamimi` を起こし直す、各 run の前の 1 分の load が 2 未満）で、`<ev> stream --wav lat-ja.wav --repeat 5 --out docs/results/rp78/<形><N>.jsonl` を `--asr-encoder cpu` と `--asr-encoder cuda` の `<hy>` で 3 対交互に流す。その後段として、split の認識器で `<ev> stages --provider cuda --asr-encoder cuda --wav lat-ja.wav --models models --repeat 5 --cache warm --only recognize --out docs/results/rp78/<条件>_split.jsonl` を RP-74 の 3 条件で 1 組ずつ（`--peers 1` の peer も split の形）、RP-74 と同じ起動の形と同じ数え方で打つ。`cargo test --release --test rp78 -- --ignored` が全部を読む | 3 対を合わせた有効な標本（3.1）で、`L_asr` の p50 が split の形で今の形より「15 ms と、今の形の 3 run の `L_asr` の p50 の幅の大きい方」以上小さく、p95 が「今の形の p95 + 今の形の 3 run の p95 の幅」以下。`L_tr` の p50 の延びが 20 ms 以内。`L` の p50 と p95 が今の形以下。後段の 3 条件の `recognize_total` の p50 と p95 を、RP-74 の同じ条件の A と B の見積りに並べて `docs/results/rp78/summary.md` に書き、無負荷と 2 音源同時で RP-74 の線を実測でも満たす（GPU の負荷の下だけが外れた時は評議へ戻す） | 今の形の 2 run を対にした時、`L_asr` の p50 の差が線より小さい（揺れが線より小さい） | encoder が CPU で作られた split の run（`gpu.session.created` の実際の provider が `cpu`）は split の run として受け付けない |
| RP-24 | build の再現（Windows と WSL） | 2 つの OS で `cargo run -p xtask -- sherpa-gpu` を打ち、`cargo test --release --test rp24 -- --ignored` が 2 つの `build.json` を比べる。2 つの OS を比べる `windows_and_wsl_builds_match` は WSL から打つ（WSL の build の directory は symlink で、Windows からは WSL が作った symlink を読めない。os error 1920） | sherpa-onnx の commit、ORT の版、CUDA の主版、cuDNN の主版、`sm_120` 系の判定（3.12）が一致。各 OS で、`sm_120` か `sm_120a` の機械語が 1 件以上、または `sm_120` の PTX があり「PTX の JIT で RP-17 が通った記録」が在る。この記録は、その build の `build.json` の sha256 を持つ `rp17_passed.json`（RP-17）である | 同じ OS で 2 回 build した `build.json` の上の項目が一致 | ORT の版を変えた build では差を名指しして落ちる |
| RP-25 | 訳の揺れ | RP-3 の Rust の run を `cargo test --release --test rp25 -- --ignored` が読む | 発話ごとに、確定の訳が出た後に表示の訳が変わった回数が 0。先の訳から確定の訳への置き換えは 1 回まで。速報の訳で、確定した字形で描いた接頭辞が後で消えた回数が 0。Zoom と Teams と SRT へ出た訳は 1 つの `id` につき 1 件で、確定の訳と同じ | 先の訳と確定の訳が違う発話の数を同じ test が数える | 確定の訳の前に先の訳を送る偽の中継で落ちる |
| RP-26 | 言語の切り替え | `cargo test --release --test rp26` が、`<hy> --help` と、fallback の時の stderr（3.15）を、locale を en と ja にして読む | en と ja で文が別の言語で、どちらも `strings.<locale>.toml` の key だけから組まれている。`strings.en.toml` と `strings.ja.toml` の key の集合が同じ | en の出力が英語 | 文言を code に直書きした build で、表に無い文が出て落ちる |
| RP-27 | GPU の経路が無い文字列の段が ORT と CUDA を呼ばない（R2） | `cargo test --release --test rp27` が `rg -c -i '\bort\b\|cuda' native/hayamimi-asr/src/itn.rs native/hayamimi-asr/src/punct/rules.rs` を子の process で打つ | 2 つの file が在り、当たりが 0 件（rc 1）。file が無いときの rc 2 は落ちる | 同じ式を `native/hayamimi-gpu/src/` に掛けると 1 件以上（rc 0） | `itn.rs` に `use ort` の 1 行を足した fixture の木で落ちる |
| RP-28 | CPU に落ちた時の主指標 `L` | RP-3 と同じ `<ev> stream --wav lat-ja.wav --repeat 5` を、`CUDA_VISIBLE_DEVICES` を空にした Rust の run で 1 回流し、`cargo test --release --test rp28 -- --ignored` が読む | GPU の run の合否（RP-3）とは別の行に、CPU に落ちた run の `L`、`L_asr`、`L_tr` の p50 と p95 を出す。仮の上限は `L` の p50 が 4500 ms、p95 が 7000 ms（R1 の「Rust 認識 cpu / 訳 cpu 無負荷」の見積り 4053 ms と 6194 ms、`gpu_stages.md` の「遅れ L の見積り」の同じ名の `L` の見積りの行、:304 に約 1 割を足した値。10 の H8）。R8e が測って置き直す | 同じ test が GPU の run からも同じ列を出せる | GPU の在る run を CPU の run として渡すと、`gpu.fallback.cpu` が 0 件なので CPU の run として受け付けない |
| RP-29 | GPU 以外の資源が無くても止まらない（3.15 の表の場面） | 3.15 の表の場面ごとに、資源を外した run と在る run を対で起こす `cargo test --release --test rp29 -- --ignored`（model の file を外した directory、llama-server を起こさない、送り先を閉じた port、8833 を先に塞いだ port、出力の装置を止めた機械、sherpa-onnx の library を外した directory、壊した設定の file）。各行は持ち主の slice が足す | 資源を外した run は、終了 code が 0、`final`（推論の library を外した行では server の `ready`）が出て、その場面の `<domain>.fallback.<落ち先>` の log の event と `/events` の `fallback` が段ごとに 1 件ずつ出る | 資源の在る run では、同じ場面の fallback の event が 0 件（在るのに落ちない） | 資源を外したのに event を出さない build で落ちる。資源が在るのに落ちる build（常に小さい model を読む build など）で、在る run の event が 1 件以上になり落ちる |
| RP-30 | `fallback` の `stage` と `reason` の値の一覧が code と同じ | `cargo test --release -p hayamimi --test rp30` が、3.15 の「`fallback` の event の `stage` の値は」の段落の 2 つの一覧と、`native/hayamimi/src/notice.rs` の `STAGES` と `REASONS` を突き合わせる（test は `#[ignore]` を持たない。docs と code の定数だけを読む） | `hayamimi` の `stage` の 13 個と `reason` の 11 個が、順を問わず集合として code と一致する。どちらかにだけ在る値は名指しして落ちる | 同じ test が、3.15 の段落から `stage` を 13 個、`reason` を 11 個読める | 一覧から 1 つ消した docs の fixture、code に無い値を 1 つ足した docs の fixture のどちらでも、その値を名指しして落ちる |
| RP-31 | `L` の数え方（3.1 の `t_tr` の条件と結び方、3.16 の 1） | `cargo test --release -p hayamimi-eval --test rp31` が、`stream` の集計の関数に fixture の event の列を渡す（`#[ignore]` を持たない） | `status` が `held` か `unavailable` の訳と `text` が空の訳は `t_tr` に入らず、同じ `id` の後の `ok` の訳が `t_tr` になる。Rust の run では `final` が `t1_s` で発話に結ばれ、次の発話の窓の中に着いた `final` も自分の発話の標本に入る。Python の run では、前の窓が「final なし」で次の窓に `final` が 2 件以上在る標本の数が別の列に出る | 同じ関数が、`held` の無い fixture で今の数え方と同じ `L` を出す | `held` の空の訳を先に置いた fixture で、空の訳の時刻を `t_tr` にする数え方を落とす。遅れて着いた `final` を着いた時刻で割る数え方を、その発話が「final なし」になることで落とす |
| RP-32 | load の記録つきで固定した Python の基準と比べた `L`（主指標、3.16 の 4） | 3.16 の 4 の形で `<ev> stream --wav lat-ja.wav --repeat 5 --out rs<N>.jsonl` を Rust で 3 本流し（`pairs.txt` が在ればその数）、Python は流さずに `docs/results/rp32/` の `py<N>.jsonl` を読む。`cargo test --release --test rp32 -- --ignored` が全 run と各 run の `/proc/loadavg` の記録と、Rust の run の `rs<N>.hayamimi.stdout.log` を読む | 各 Rust の run の前の 1 分の load が 2 未満。流した順は見ない。Python の基準の run と Rust の run を合わせた標本で、Rust の `L` の p50 と p95 が Python の p50 と p95 以下（RP-31 の数え方）。有効の線は 3.1 の終端不正の例外のとおりで、Python の標本は `min_silence`、Rust の標本はその標本を閉じた `final` の `asr.endpoint.decided` の `rule` が `early` なら `EARLY_MIN_SILENCE_MS`（`native/hayamimi-asr/src/vad.rs` の定数から取り、数を書かない）、それ以外なら `min_silence`。`asr.endpoint.decided` に結べない Rust の `final` が 1 件でも在れば、その run と `id` を挙げて値を出さずに落ちる。全標本を `min_silence` で数えた対の比較の数と `L` の p50 と p95 も、記録の列に並べて出す（3.16 の 4 の「RP-32 の報告」）。RP-3 の対の比較（17 発話以上）も同じ標本で数える。Rust の有効な標本のうち訳の取れた割合が 20 / 23 以上で（E2E-1 と同じ値）、分母は発話の標本（manifest の発話 × `--repeat` の回。1 つの run で 23 × 5）のうち 3.1 の「有効」に入る物の数とし、`final` の数や訳の依頼の数では数えない。`held` だけの標本と、`t1_s` で自分の発話に結んだ遅れた `final` の標本は、有効なら分母に入り、`t_tr` が取れなければ分子に入らない。有効の判定（3.1 の分割式）は RP-3 の test から `hayamimi-eval` の集計の関数へ移し、標本ごとに線を受ける形にして、RP-3 と RP-32 が同じ関数を使う。RP-3、E2E-1、E2E-2 は全標本に `min_silence` を渡し、RP-32 は上の規則の線を渡す。Python の同じ割合からの欠けも別の列に出す。訳の取れない標本は p50 と p95 から外れ、V が止めるほど `L` が小さく出るので、割合を合否に入れる。`L_asr` と `L_tr` と、「訳」の段（`final` から `t_tr` まで）の p50 と p95 を別の列に出す。`finals` が 2 以上の標本の割合、ja の CER、「final なし」の数は品質の記録の列で、合否に入れない（3.16 の 4 の「RP-32 の報告」） | 同じ test が Python 同士の 2 run の対から同じ列を出せる。全 `final` の `rule` が `silence` の log を渡すと、行ごとの線で数えた有効な標本の数と `L` の p50 と p95 が、全標本を `min_silence` で数えた値と同じになる | load の記録が無い run、1 分の load が 2 以上の Rust の run のどれかが在れば、値を出さずに落ちる。訳の取れた割合が 20 / 23 未満の Rust の run（R8e の記録の 67 / 93 の形）は、`L` が Python 以下でも落ちる。`rule` が `silence` で `L_asr − L_dec` が `EARLY_MIN_SILENCE_MS` 以上 `min_silence` 未満の Rust の標本は終端不正に入り、それを有効に入れる判定（全標本を早い線で数える形）は落ちる。`asr.endpoint.decided` の行を 1 つ消した log では、その `final` の `id` を挙げて落ちる |
| RP-33 | T が V に落ちた行の最初の訳（3.16 の 2） | RP-32 の Rust の run を `cargo test --release --test rp33 -- --ignored` が読み、中継の stdout の行ごとの記録で、T の訳が V に落ちた行と通った行に分ける | 「訳」の段の p50 と p95 が、全行で Python の「訳」の段の p50 と p95 以下。T が V に落ちた行だけの p50 と p95 と行の数を別の列に出す。B（R8f の V の直し）の後の run で打つ。A を作った時は、T が V を通った行の `L` の p50 と p95 が A の前の run 以下であることも足す | 同じ test が、T が V を通った行だけの列も出せる | T が V に落ちた行の最初の訳を 600 ms 遅らせる偽の中継で落ちる |
| RP-34 | 確定の復号の前の待ち（3.16 の 3） | RP-32 の Rust と Python の run を `cargo test --release --test rp34 -- --ignored` が読み、`final` ごとに `L_asr − decode_ms` を出し、発話の長さ（4 秒未満、4〜7 秒、7 秒以上）で分ける。Rust の run は window を受けてから feed を始めるまでの待ちの log と、各 run の前後の `nvidia-smi` の `utilization.gpu` と `memory.used` の記録も読む | 3 つの長さの組のそれぞれで、Rust の `L_asr − decode_ms` の p50 が Python 以下。`decode_ms` の p50 と待ちの log の p50 は記録の列で、合否に使わない。GPU の負荷の記録が Python と Rust の対で違う対は無効にし、その数を出す（Python の `decode_ms` の意味は確かめていない。10 の H43）。速報は RP-5 を満たす | 同じ test が、速報を切った Rust の run からも同じ列を出せる | 確定の復号の前に 300 ms の待ちを入れた偽の engine の run で、3 つの組のどれかが落ちる。GPU の記録が無い run で、値を出さずに落ちる |
| RP-35 | `L` の詰めで品質を下げない（3.16 の品質の条件。2 の B を入れた時と、A を作った時の両方で打つ） | R8h の前の main と R8h の branch で、同じ llama-server と model に `<relay> eval --pairs <held-out の組の dir> --out <hyps の dir>`（今の code が受ける引数は `--pairs`、`--out`、`--llm`、`--llm-other` の 4 つで、原文は ja だけ。`native/hayamimi-relay/src/cli.rs` の `parse_eval`）を打ち、`<ev> chrf --pairs <同じ dir> --hyps <hyps の dir>` で採点し、RP-13 の en の行を打ち、`cargo test --release --test rp35 -- --ignored` が 2 組の結果と RP-32 の Rust の run の中継の stdout を読む。en→ja の held-out の入口と、組を 1 回だけ使う拒み（CS-30）は未実装で、8 の R12y が持つ。R12y が入るまで、en→ja の held-out の行と CS-30 の拒みは打てない | held-out の chrF が ja→en で前の main 以上（en→ja は R12y の後）、V が止めた行の数が前の main 以下（止める行が増えない）、RP-13 の en の chrF が前の main 以上。RP-32 の run で、`status` が `ok` の訳の全件に、その訳の文字列に掛けた V の `ok` の記録が在る。3 を入れた時は RP-1 の ja の CER が前の main 以下 | 同じ test が前の main 同士の 2 組から差 0 を出す | E0（または E）の応答に V を掛けずに出す偽の中継で、V の記録の無い `ok` の訳が 1 件以上になり落ちる。T の訳をそのまま確定にする偽の中継で held-out の chrF が下がり落ちる |
| RP-36 | RP-1 の ko の R11 の入口の上限（3.2 の 2） | `cargo test --release --test rp36 -- --ignored` が、Windows の配る形の build で振り分けを通した run の記録 `docs/results/r3c/win_routed.json`（git の中）を読む。ORT か sherpa-onnx の版を上げて測り直した時は、その run の記録でこの file を置き換えてから打つ | ko の誤り率の平均を百万分率の整数で丸めずに出し、86939 以下。超えたら評議へ出す（RP-1 の閾値 0.081 の判定は RP-1 のまま） | 同じ test が、Python の wheel の ORT を読ませた run の記録（`win_python_libs.json`）から ko 80572 を出す | ko の平均を 86940 にした記録で落ちる |
| RP-37 | zh、ko、es の V の推量を log にだけ残す（3.6 の R8i） | `cargo test --release -p hayamimi-relay --test rp37` が偽の llama-server の応答を V に通す。merge の前に dev の seed 0 と seed 1 の組で `hayamimi-relay vprobe --pairs <dir> --lang <en|zh|ko|es>`（`--out` なし）を今の main と branch で打つ。`<dir>` は seed 0 が `testdata/baseline/pairs/`、seed 1 が `testdata/baseline/pairs_seed1/`（git の外。dev の ja と対象の言語の重なりの全行から seed 0 の組の id を除いた残りで、en、zh、ko、es が 85、85、68、85 行、seed 0 との id の重なり 0）。seed 1 の組は R8i の実装の席が作った物をこの場所へ写して置き、置いた時に言語ごとの ids の sha256 を state.json に書く。seed 1 の出所は、作っていない検証の席が、言語ごとに `pairs_seed1` の id の集合が「dev の ja と対象の言語の重なりの id の集合から seed 0 の id を除いた集合」と同じであることを `jq` で照らし、差の数（0 であること）を返す。dev の id は、`fleurs.rs` の `tsv_url` が返す dev の表（`https://huggingface.co/datasets/google/fleurs/resolve/main/data/<config>/dev.tsv`）の 1 列目を ja_jp と対象の言語の config ごとに取り、その検証の席が `testdata/baseline/fleurs_dev_ids/<config>.ids.txt`（git の外）に置いてから照らす | zh、ko、es で推量の語だけが原文と食い違う訳は止まらずに出て、`translate.hedge.logged` が 1 件出る。ja→en の同じ食い違いは止まらずに log に残る（8 の R8s の 1、RP-48）。en→ja は、原文の推量が訳で落ちた時だけ `uncertainty` で止まり、訳だけが推量を持つ時は止まらずに `translate.hedge.logged` が 1 件出る（8 の R8t、RP-63）。zh、ko、es でも、否定を落とした訳は `negation` で、数を変えた訳は数の理由で止まる。vprobe では、数と否定の壊した行の止まる数が今の main 以上、参照訳の止まる数が今の main 以下 | 同じ test が、推量の食い違いの無い zh の訳を止めずに出し、`translate.hedge.logged` が 0 件 | zh で推量の食い違いを止める build、en→ja で原文の推量を落とした訳を止めない build、zh で否定と数を止めない build のどれでも落ちる |
| RP-38 | `eval` は 1 行に `translate.hedge.logged` を 1 件だけ出す、の回帰（3.6 の R8i、会話字幕の設計 3.13） | `cargo test --release -p hayamimi-relay --test rp38` が偽の llama-server で `hayamimi-relay eval` の経路に推量の食い違いの在る zh の 1 行を流す。zh、ko、es は `eval.rs` の `settle_other` で V を 1 回だけ掛けるので、今の code はこの形を満たしている | 同じ行の `translate.hedge.logged` が 1 件 | 同じ test が、推量の食い違いの無い行で 0 件を出す | `settle_other` の V の前に 1 行足して V を 2 回掛けた build で、2 件になり落ちる |
| RP-39 | zh、ko、es の倍の語を読む V の数の検査（3.6 の R8l） | `cargo test --release -p hayamimi-relay --test rp39` が `numbers.rs` の読みと、偽の llama-server の応答を V に通す。merge の前に dev の seed 0 と seed 1 の組で `hayamimi-relay vprobe`（`--out` なし）を今の main と branch で打つ | 3.6 の R8l の例の値を全部その数で読む。`予算は五百万円です。` を zh `预算是5百万日元。`、ko `예산은 5백만 엔이에요.`、es `El presupuesto es de 5 millones de yenes.` へ訳した行が止まらずに出る。vprobe では参照訳の止まる数が今の main 以下、数の壊した行の止まる数が今の main 以上 | 同じ test が、日本語と英語の今の読み（`500万`、`5 million`）を同じ値で読む | 数を変えた訳（zh `7百万`、ko `7백만`、es `7 millones`）が `number` で止まらない build で落ちる。倍の語を読まない今の `numbers.rs` で、`5百万` の行が止まり落ちる |
| RP-40 | 字幕の頁と中継の SSE に LAN から届く（8 の R5b） | `cargo test --release -p hayamimi --test rp40 -- --ignored` が、`<hy> --wav tests/golden/ja/ja_014.wav --serve 0 --serve-host 0.0.0.0` と `<relay> live --serve 0 --serve-host 0.0.0.0` を起こし、その機械の loopback でない IPv4 の address（test が UDP の socket の local address で引く）へ `GET /` と `GET /events` を打つ | `hayamimi` の `GET /` が 200 で、中継は `GET /events` が `text/event-stream` を返す（中継の `/` は頁を持たず 404 のまま）。`hayamimi` の `/events` も `text/event-stream` を返す。起動の stderr に、その address の URL が 1 行出る | 同じ test が `127.0.0.1` に打っても同じ応答を返す | `--serve-host` を付けない run（既定 `127.0.0.1`）では、同じ LAN の address に繋がらず落ちる側を確かめる（繋がれば落ちる） |
| RP-41 | `hayamimi-eval stream` の `listen` が Rust の `/events` を SSE で読む（8 の R8m） | `cargo test --release -p hayamimi-eval --test rp41` が、偽の SSE の server（`text/event-stream` で `final` と `translation` を 1 件ずつ送る）に `listen` を向ける | `listen` が 2 件の event を受け、時刻を記録する | 同じ test が、`<hy> --serve 0` の本物の `/events` からも 1 件以上の event を受ける（`--ignored` の行） | WebSocket で繋ぐ今の `listen` で、偽の SSE の server に繋げず落ちる |
| RP-42 | 両方止めた行の理由が log で読める（8 の R8o の 2、観測だけ） | `cargo test --release -p hayamimi-relay --test rp42` が、偽の llama-server で T と E の両方が V に止められる ja→en の 1 行を中継に流す | `translate.line.held` に、E の側の理由に加えて T の側の理由（V の理由と同じ enum の値）が載る。log には止めた訳の本文が載らない。`Observer` の `draft_verify_input` と `verify_input` が受けた 2 つの訳の文字列は、repo の外の dev 用の file（起こす側が path を渡した時だけ）に 1 行ずつ出る | 同じ test が、T が V を通る行では T の側の理由を載せない | T の側の理由を載せない build、log に止めた訳の本文を載せる build のどちらでも落ちる |
| RP-43 | 否定の表に「なさそう」「なさげ」「なさすぎ」（8 の R8o の 1） | `cargo test --release -p hayamimi-relay --test rp43` が V に原文と訳を渡す。merge の前に、同じ run で E2E-1 を打ち直す | 原文の「なさそう」「なさげ」「なさすぎ」が否定の語と読まれ、否定を残した en の訳は止まらず、否定を落とした訳は `negation` で止まる。「なさい」「なさる」「ごめんなさい」は否定の語と読まない。E2E-1 の打ち直しで、Rust の訳の取れた標本が 70/80 以上（utt 16 の 4 件のうち 3 件以上が戻る）。71/80 は見込みで確かめていない | 同じ test が、今の表の「ない」の行を今のとおり読む | 「なさ」だけを表に足した build で「ごめんなさい」の行が `negation` で止まり落ちる。表を足さない今の build で「なさそう」の行が落ちる |
| RP-44 | R8o の後の RP-35 の held-out の組（新しく作る） | test-designer の席が repo の外に新しい組を作り、`cargo test --release -p hayamimi-eval --test rp44 -- --ignored` が組の ids と記録を読む | dev と test split の id との重なりが 0 で、原文に「なさそう」を含む行が 1 行以上在る。言語ごとの ids の sha256 を state.json に書く。組は 1 回だけ使う（会話字幕の CS-30 は同じ `heldout_sha256` の 2 回目を拒む）。組が届くまでの RP-35 の代わりは、dev の seed 0 と seed 1 の vprobe（RP-37 の形）である。RP-35 の chrF の平均は、組のうち FLEURS の表から取った行だけで出す。手書きの行は参照訳が 1 本で書き手が 1 人なので、chrF の平均に入れず、V が止めた行の数（RP-35）と否定の語の確かめにだけ使う。全行の chrF は別の列に記録だけを置く。組は RP-35 の前の main と branch の比べにだけ使い、記録の値（RB-7、RP-13）と比べない。評価の model が FLEURS の文を学習で見ていれば 2 つの腕の chrF が同じ向きに上がるので差には効きにくいが、記録の値との比べでは model の側にだけ効くからである。2 つの model の技術報告は学習のデータに FLEURS と FLoRes の名を挙げないが、web の crawl を使うので、見ていないとは言えない（S21、S22。NOT VERIFIED）。前の main の腕も同じ組で打ち直し、別の組の結果と比べない。2 つの腕は別々の結果の file に書く（CS-30 が拒むのは、同じ結果の file に同じ組が別の表で在る時である。1 つの判定の 2 つの腕を 1 回の使用とする。CS-30 の拒みは今の code に無く、8 の R12y が持つ） | 同じ test が、dev の seed 0 の組の ids から重なりを数えられる | dev の id を 1 つ混ぜた ids で落ちる |
| RP-45 | E の雛形が字幕に出ない（8 の R8p） | `cargo test --release -p hayamimi-relay --test rp45` が、偽の llama-server の E の答えに雛形の札（`English:`、`Draft Japanese:`、`Rewritten Japanese:`、ja→en では同じ形の札）を付けて中継に流す。merge の後の E2E-2 の打ち直しで、確定の訳の文字列を数える | 札は 8 の R8p の形（行頭の `Draft` 系か `Rewritten` に、訳の言語名と `:` が続く形）だけで、最後の札の後ろの訳だけが確定の訳になり、その札の語は確定の訳に残らない。行頭に無い札の形（太字、言語名と `:` の間の注、番号つき、1 行に続く雛形）は剥がさず、訳の本文として V に渡す（10 の H60）。札しか無い答え、札を剥がすと空の答えは `form` で止まる。E2E-2 の打ち直しで、確定の訳のうち札の語を含む物が、行頭か否かを問わず 0 件（1 件でも在れば H60 の形が run に出たので、その形を名指しして設計へ戻す） | 同じ test が、札の無い E の答えをそのまま確定にする | `edit` で `translation_of` を外した build で、札の付いた訳が確定になり落ちる。札を行の途中でも読む build で、札に似た正しい訳（`The draft: nobody read it.`）が削られて落ちる |
| RP-46 | 英語の認識器を最初の英語の発話より前に読む（8 の R8q） | `cargo test --release -p hayamimi --test rp46 -- --ignored` が、`<hy> --input ws --lang en --ws-port <空いている port>` を起こし（`--ws-port 0` は選んだ port を知らせないので、RP-11 と同じく空いていた port を渡す）、server が listen した直後に繋いで `/ingest` に英語の音声を 2 発話送る | `asr.model.loaded`（`model` が `v3`）が `/ingest` の `ready` より前に出る（`ready` を返した直後に出る `ingest.client.connected` の行より前の行で見る）。最初の発話の `final` の `decode_ms` が読み込みの時間を含まない（`asr.model.loaded` の `load_ms` より小さく、2 発話目の `decode_ms` の 10 倍以下） | 同じ test が `--lang ja` の run で ReazonSpeech の読み込みを `ready` より前に見て、英語の認識器（`v3`）の `asr.model.loaded` が 0 件 | 最初の英語の発話で読み込む今の形で、`asr.model.loaded` が `ready` より後になり落ちる |
| RP-55 | session の reset が前の session を残さない（8 の R12h） | `cargo test --release -p hayamimi-asr --lib reset_parts` | `reset_parts` の後に、`History` の buffer が空で、次の `with_preroll` が reset の前の sample を 1 つも含まず、`Router` の言語の粘りが無く（次の発話の振り分けが最初の発話と同じ扱い）、話者の重心が 0 個。`History` の絶対の位置（`end()`）は reset の前の値のまま | 同じ test が、reset を打たない run で前の session の preroll と言語と重心が残ることを示す | `History::clear` を呼ばない `reset_parts` で、preroll に前の sample が入り落ちる。`Router::reset_session` を呼ばない形で、言語の粘りが残り落ちる |
| RP-56 | `final` の `switched` が前の `final` の言語と比べて決まる（8 の R12i） | `cargo test --release -p hayamimi --lib switched` | 最初の `final` で偽、言語が変わった `final` で真、同じ言語が続くと偽。session の reset の後の最初の `final` で偽 | 同じ test が、言語の同じ 2 本で偽を 2 回出す | `switched` を常に偽で出す今の `wire.rs` で、言語が変わった行が落ちる |
| RP-57 | DER の内訳（8 の R12q） | `cargo test --release -p hayamimi-eval --lib der_breakdown` | 同じ ref と hyp で Miss、FA、Confusion が全部 0。話者を入れ替えた hyp で Confusion だけが増え、区間を落とした hyp で Miss だけが増える。3 つの和が `der` の値と 1e-9 以内で同じ | 同じ test が、内訳の和を `der` の値と比べる | Confusion を Miss に足す内訳で、入れ替えの行が落ちる |
| RP-58 | Windows の COM の初期化の HRESULT を分ける（8 の R12r、3.7） | `cargo test --release -p hayamimi --lib windows_com_scope` | HRESULT を分ける純関数が、成功（`S_OK`、`S_FALSE`）を後始末の要る初期化、`RPC_E_CHANGED_MODE` を後始末しない既存の初期化、それ以外を endpoint の名前を持つ `CaptureError` に分ける。scope の型は成功の時だけ後始末を 1 回呼ぶ | 純関数の test は Windows でなくても走る | `RPC_E_CHANGED_MODE` を失敗として返す今の形（`windows.rs` の `run`）で落ちる |
| RP-59 | 4 クラスの句読点（8 の R12t1〜R12t3、3.10 の `--punct-4class`） | `cargo test --release -p hayamimi-asr --lib punct::four`、model を使う行は `-- --ignored` | T の 5.3 の `test_punct_4class.py` の 14 行と、`--punct-model` の選択の 7 行が通る。既定は BERT のままで、4 クラスの model が無いときは BERT へ落ちて warn を 1 回出す | 同じ test が既定の BERT の経路で今と同じ文字列を出す（RP-18 が通ったまま） | 4 クラスを既定にした build で、既定の行が落ちる |
| RP-61 | 清書を live に足しても `L` を延ばさない（8 の R12w4） | 清書を入れた `<hy>` と切った `<hy>` で RP-32 の順の run（rp03）と E2E-1、E2E-2 を打ち直し、`cargo test --release --test rp61 -- --ignored` が読む | 清書を入れた Rust の `L` の p50 と p95 が、同じ負荷の Python（清書を既定のまま）の p50 と p95 以下。清書を切った Rust との差が p50 で 30 ms 以下、p95 で 60 ms 以下（仮の値、10 の H64）。訳の取れた割合が 20 / 23 以上。`refine` の event が 1 件以上 | 同じ test が、清書を切った 2 run から差 0 を出す | 清書を live と同じ `Router` で解く build で、確定が group の解き直しを待ち、p95 の差が 60 ms を超えて落ちる |
| RP-62 | recluster の音声の寄せ集め（8 の R12x） | `cargo test --release -p hayamimi-asr --lib recluster` | T の 5.3 の `pool_audio_for_group_*` の 4 行が通る | 同じ test が、turn の在る group で turn の音声だけを返す | turn を見ずに buffer の全部を返す関数で、turn の行が落ちる |
| RP-60 | 2 人の話者の材料（8 の R12u1） | `cargo test --release -p hayamimi-eval --lib two_speaker_clip` | 8 の小節の規則で `testdata/eval_diar/` の RTTM から窓を 1 つ選び、窓の中の話者が 2 人、区間が 4 つ、少ない方の区間が 1 つで 2〜4 秒、重なりが 0。manifest に会議の名前、始まり、終わり、sha256 が在る | 同じ関数が、規則に合う窓を手で置いた RTTM の fixture から同じ窓を選ぶ | 重なりの在る窓だけの fixture で、窓が無いと名指しして `Err` を返す |
| RP-47 | 新しい確定文の T を前の行の E の後ろに並べない（8 の R8r） | `cargo test --release -p hayamimi-relay --test rp47` が、E に 1000 ms の待ちを入れた偽の llama-server に、200 ms 間を空けた 2 つの確定文を流す | 2 つ目の確定文の T の依頼が、1 つ目の E の応答より前に出る。slot の数（3.6 の 5）を超えて依頼を並べない | 同じ test が、E の無い形（zh）で 2 つの T が順に出ることを示す | 訳の lane が 1 本で直列の今の形で、2 つ目の T が 1 つ目の E の後になり落ちる |
| RP-48 | ja→en の推量の不一致を止めず log にだけ残す（8 の R8s の 1） | `cargo test --release -p hayamimi-relay --test rp48` が偽の llama-server の応答を V に通す。merge の前に、dev の seed 0 と seed 1 の組で `hayamimi-relay vprobe --pairs <dir> --lang en`（`--out` なし。`<dir>` は RP-37 と同じ）を今の main と branch で打つ | ja→en で推量の語だけが原文と食い違う訳は止まらずに出て、`translate.hedge.logged` が 1 件出る（`eval` でも 1 行に 1 件。V を 2 回掛ける `settle_pair` は確定の訳の V の時だけ出す。10 の H48）。en→ja は、原文の推量が訳で落ちた時だけ `uncertainty` で止まり、訳だけが推量を持つ時は止まらずに `translate.hedge.logged` が 1 件出る（8 の R8t、RP-63）。受入 test の名前の `into_japanese_held` は、原文の推量を落とした en→ja の訳が止まる形を指す。`feat/r8s` の test は検証済みの名前なので名前は変えず、R8t の実装の席が、その test の en→ja の行を原文の推量を落とした訳に書き直す。ja→en でも、否定を落とした訳は `negation` で、数を変えた訳は数の理由で止まる。vprobe では、数と否定の壊した行の止まる数が今の main 以上で、推量の止まりが減った数を言語の行ごとに記録する（合否は前の 2 つの不等号で決める） | 同じ test が、推量の食い違いの無い ja→en の訳を止めずに出し、`translate.hedge.logged` が 0 件 | ja→en で推量の食い違いを止める今の build、en→ja で原文の推量を落とした訳を止めない build、ja→en で否定と数を止めない build のどれでも落ちる |
| RP-49 | 語で書いた半時（`N時半`、`half past N`）が持つ 30 を読む（8 の R8s の 2） | `cargo test --release -p hayamimi-relay --test rp49` が V に原文と訳を渡す。merge の前に dev の seed 0 と seed 1 の組で `hayamimi-relay vprobe --lang en`（`--out` なし）を今の main と branch で打つ | ja と en の間の V の数の検査（両方向）で、ja の `N時半` と en の `half past N` に 30 を 1 つ足して読む。`one thirty` は今のとおり 1 と 30 と読み、時と分の組には読まない。`1時半に会いましょう。`→`Let's meet at one thirty.` は ja→en でも en→ja でも止まらずに出る。`1時30分`→`at one thirty` と `half past one`→`1時半` も止まらない。`two thirty` を `1時半` の訳にした行と、分の違う訳は数の理由で止まる。zh、ko、es への訳には 30 を足さない。半時の語の外の数の読み（`thirty-first` の 31、`5 million`）は今のまま。vprobe の数の壊した行の止まる数は 33/33 のまま | 同じ test が、半時の語を持たない今の数の読みを同じ値で読む | 半時の 30 を読まない今の `numbers.rs` で、`At one thirty` の行が止まり落ちる（実装の前に今の main で止まることを再現した。止まる理由は、`1時半` の側に 30 が無く `one thirty` の側だけが 30 を持つことである）。単位の語の次に十の位の語を一般に続けて読む build（`one thirty` を 31 と読む）で、`1時半` の行が数の理由で止まり落ちる |
| RP-63 | en→ja の推量は原文の推量が落ちた時だけ止め、義務の過去形と「なくては」の形を否定と読まない（8 の R8t） | `cargo test --release -p hayamimi-relay --test rp63` が V に原文と訳を渡し、偽の llama-server の応答を中継に通す。merge の前に dev の seed 0 と seed 1 の組で `hayamimi-relay vprobe --lang en`（`--out` なし）を今の main と branch で打つ | 推量：en→ja で、原文が推量を持ち訳が持たない行（`I might go.`→`行きます。`）は `uncertainty` で止まる。原文が推量を持たず訳だけが持つ行（`It will rain.`→`雨が降るでしょう。`、`I could go.`→`行けるかもしれません。`。`could` は en の推量の表に無い）は止まらずに出て、`translate.hedge.logged` が 1 件出る。その event の `lang` は訳の先の言語の `ja` である（`verify.rs` の `hedge_event`）。`eval` は原文を ja としてしか読まず（`eval.rs` の `verify_into` に `Lang::Ja` を渡す）、en→ja の入口を持たないので、en→ja は偽の llama-server を通す `Pipeline::run` で確かめる。両方が推量を持つ行（`I might go.`→`行くかもしれません。`）は今のとおり出る。否定：`ja.negation` の `exclude` に、`なければならなかった`、`ないといけなかった`、`なくてはならない`、`なくてはなりません`、`なくてはならなかった`、`なくてはいけない`、`なくてはいけません`、`なくてはいけなかった` の 8 つを足す。この 8 つの形の原文の、否定を持たない正しい訳（`宿題をしなければならなかった。`→`I had to do my homework.`）は止まらない。同じ原文の義務を落とした否定の訳と、本当の否定（`宿題をしなかった。`→`I did my homework.`）は今のとおり `negation` で止まる。vprobe では、数と否定の壊した行の止まる数が今の main 以上 | 同じ test が、8 つの形を含まない今の否定の行（`それは美味しくない。`）を今のとおり読む | 訳だけの推量も止める今の build で `雨が降るでしょう。` の行が落ちる。原文の推量を落とした訳を止めない build で `行きます。` の行が落ちる。8 つを足さない今の build で `なければならなかった` の行が `negation` で止まり落ちる。`ならなかった` を単独で除く build で、本当の否定（`雨にならなかった。`→`It rained.`）が止まらず落ちる |
| RP-64 | 欠番。T と E の両方が数で止まった行を訳し直す形（8 の R8u）は、probe で効かないと分かったので作らない（10 の H66）。その行は held のまま訳を出さず、原文の字幕を出し続ける。理由が log で読めることは RP-42 が測る | なし | なし | なし | なし |
| RP-65 | session の言語の文字を持たない 1〜2 字の確定文を出さない（8 の R3e） | `cargo test --release -p hayamimi --test rp65` が、確定文を出す所に偽の認識の結果を渡す | 前後の空白を除いて 1〜2 字で、どの字も session の言語の文字（en は Latin、ja は漢字、ひらがな、カタカナ、Latin）に入らない確定文（`--lang en` の `ل    `）は、`final` を出さず `asr.final.dropped`（info、`reason` は `off_script`）を 1 件出す。捨てるのは、1〜2 字のうちに文字（Unicode の alphabetic）が 1 つ以上在り、そのどれも session の言語の文字でない時だけである。3 字以上の文、session の言語の文字を含む 1〜2 字の文（en の `OK`、ja の `はい`）、文字を持たない数字と記号だけの 1〜2 字（`42`、`?`）は今のとおり出す（数字と記号はどの言語の発話でも本物でありうるので、落とさない側に倒す）。空白だけの確定文は字が 0 でこの規則の外に在り、今の扱いのまま変えない | 同じ test が、`ل` を含まない短い確定文を全部出す。RP-1 の scorecard の run と golden の 8 本（RP-2）で `asr.final.dropped` が 0 件（本物の音声の確定文を落とさない） | 字の数だけで捨てる build で `OK` と `はい` が消えて落ちる。捨てない今の build で `ل` の `final` が出て落ちる |
| RP-66 | 発話の終わりの待ち `d`（`L_asr − L_dec`）と、区切りの品質（3.17、8 の R8v） | E2E-1（`lat-ja`）と E2E-2（`lat-en`）の手順で、R8v の前の main の `<hy>`（`--endpoint` を持たない build）と R8v の `<hy>`（既定の `--endpoint early`）を、main、R8v、main、R8v の順に言語ごとに 2 対流す。run の前の 1 分の load が 2 未満であること、`/proc/loadavg`、中継の stdout、`hayamimi` の log を run の file と同じ所に残すことは RP-32 と同じ。記録は `docs/results/r8v/` に置き、`cargo test --release -p hayamimi-eval --test rp66 -- --ignored` が読む | 遅れの合否は `d` だけで見る。有効な標本（3.1 の終端不正の例外のとおり `min_silence` を 0.12 秒にして数える）で、R8v の `d` が ja で p50 500 ms 以下かつ p95 1050 ms 以下、en で p50 300 ms 以下かつ p95 520 ms 以下。ja の p95 の限度 1050 ms は見積り（10 の H68）から置いた仮の値で、R8v の手番 1 の probe の結果を見て、手番 3 の run の前に置き直す（置き直した値と理由は `docs/results/r8v/probe.md` に書く）。品質の合否は 2 つで見る。1 つ目は、B だけが作った割れ（3.17 の方針 4）を 3.17 の「RP-66 と RP-67 の数え方」の 8 と 9 で仕分けた**文の途中の切断の率**が上限以下であること。分母は枠の全標本（発話の数 × 回の数。数え方の 3 と同じ）、分子は ja の参照で「文の途中」と仕分けた割れの数、上限は ja 10 %（手番 1c の 80 本では 8 件まで、9 件で不合格）。en の率は記録だけで、合否に入れない。2 つ目は、「final なし」の標本の数と訳の取れない有効な標本の数が同じ対の main より増えず、ja の CER（run の全標本の `cer_dist` の和 ÷ `cer_denom` の和）が対の main 以下であること。`finals` が 2 以上の標本の数、仕分けの 3 つの数（本物の文の切れ目、文の途中、尾の雑音）、割れた標本の訳の chrF（数え方の 8）は記録するだけで合否に入れない。`L`、復号（`L_dec`）、訳（`L_tr`）は p50 と p95 を記録するだけで、合否に入れない（`L` の p95 の合否は RP-67） | 合わせた 2 本の main の run の `d` の p50 が、今の main の記録（ja 865 ms、en 457 ms。3.17 の「RP-66 と RP-67 の数え方」の 1）から ±60 ms に入る。R8v の run の `hayamimi` の log に `asr.endpoint.decided` の `rule` が `early` の行が 1 件以上在る | R8v の build を `--endpoint silence` で流した run が `d` の期待値を外して落ちる。完結の判定を常に真にした build（毎回の候補で確定する）が、文の途中の切断の率で落ちる |
| RP-67 | 主指標 `L` の p95 を 1000 ms 以下にする（3.17、10 の H69） | E2E-1（`lat-ja`、23 発話 × 5 回）と E2E-2（`lat-en`、15 発話 × 5 回）を、E2E の手順（`docs/results/e2e/README.md` の条件）で流し、`cargo test --release -p hayamimi-eval --test rp67 -- --ignored` が `docs/results/rp67/` の `e2e1.jsonl` と `e2e2.jsonl` を読む | 3.1 の数え方（訳の取れた有効な標本、nearest-rank。有効は 3.1 の終端不正の式で `min_silence` を 0.12 秒として数える（3.17 の「RP-66 と RP-67 の数え方」の 3、H70））で、E2E-1 と E2E-2 のそれぞれの `L` の p95 が 1000 ms 以下。`d`、復号、訳の p50 と p95 を同じ結果に並べて書く。`L` は `t_end`（manifest の `end_sample`）から数え、起点を動かさない。合否が起点の確かさ（3.1）の組で割れないかは RP-73 が見る。持ち主は 2 つに分ける。ja（E2E-1）は訳の波（3.16 の 2、R8h の 2）で、R8f の V の直しは main に入っており、残る手は RP-32 の Rust の run（R8h の 4）と、3.18（R8w）の訳の段の詰めと、名前だけ在って repo に無い RP-33 の test（R8n）である。A（T と E0 を並べて投げる形）は作らない（3.16 の 2）。en（E2E-2）の持ち主は R7 の席（認識の遅れの詰め、8 の R7 の行）で、復号の p95（422 ms）と、確定の解き直しを省く H10 から始める | test が、今の main の記録（`/home/fukugan/.cache/hayamimi-e2e-20261015/run3/` の e2e1 と e2e2）を読むと、ja 1871 ms、en 1303 ms で落ちる。同じ test が、全標本の `L` を 900 ms に置いた fixture で通る | `L` が 1000 ms を超える標本を、許す数（n から nearest-rank の順位を引いた数）より 1 件多く入れた fixture で落ちる |
| RP-68 | 流れの途中で VAD の `flush` を打つと detector が abort する形の再現と、R8v と `finish` の後の feed がその形に入らない守り（3.17 の方針 4、10 の H75） | `cargo test --release -p hayamimi-asr --test rp68`。abort は test の harness ごと殺すので、親の test が自分の test の binary（`std::env::current_exe`）を子 process として引数つき（`--ignored --exact <子の test の名前>`）で起こし、子の終わり方（終了の status と stderr）だけを見る。音声は `testdata/lat-ja.wav` の 1 発話の声の区間を切って使う | 子 1（再現）：sherpa-onnx の `VoiceActivityDetector` を直に開き、「発話 → 0.12〜0.2 秒の無音 → `flush` → 0.30 秒以上の無音」を 512 sample ずつ食わせると、子が正常に終わらず、stderr に `Invalid n` が出る。子 2（守り、R8v の形）：同じ音声を `hayamimi-asr` の `Vad`（A と B）と確定の線の規則に通すと、子が status 0 で終わり、final が 1 件で、その `t1_s` が確定の線の sample である。子 3（守り、`finish` の後）：同じ音声の後に `Engine::finish` を打ち、続けて同じ音声をもう 1 回食わせても、子が status 0 で終わる | 子 2 と子 3 が status 0 で終わる（守りの形が abort しない） | 子 1 が status 0 で終わる時（固定の版で abort しなくなった時）は、この test が落ちて再現の前提が変わったことを言う。子 2 を、確定の時に A へ `flush` を打つ build で流すと落ちる |
| RP-69 | 部分訳の使い回し（3.18、8 の R8w） | `cargo test --release -p hayamimi-relay --test rp69` が、T の答えに 300 ms の待ちを入れて依頼の数を数える偽の llama-server に、偽の認識器から速報と `final` の列を流す（`#[ignore]` を持たない） | (a) 速報 `X` の訳が V を通って出た後に、同じ `id` の `final` `X。` が来ると、その行の T の依頼は 0 本で、`final` を受けてから 50 ms 以内に、`of` が `final`、`stage` が `t`、`settled` が偽、`src_sha256` が `X。` の sha256 の訳が出る。`translate.draft.reused` が 1 件（`match` が `closing_mark`、`state` が `done`）。(b) 速報 `X` の依頼が走っている間に `final` `X。` が来ると、その依頼は打ち切られず、その発話の T の依頼は全部で 1 本で、先の訳は速報の答えが来た時に出る（`state` が `running`）。その後に次の発話の速報が来ても、その依頼は打ち切られない。(c) 速報の T の答えが原文に無い数を持つ時、`final` `X。` の行は T を頼まずに E を頼み、先の訳を出さない。(d) 使い回した答えに掛けた V の原文は `final` の文字列である（observer の `draft_verify_input` と V の入力で数える）。(e) 使い回した行の V は 1 行に多くて 2 回である | 同じ test が、`final` が `X？` の時と、`id` の違う `final` の時に、T を 1 本頼み直し、`translate.draft.reused` が 0 件であることを示す | `final` が来ると速報の依頼を止める今の形で、(a) と (b) の T が 2 本になり落ちる。句読点を全部除いて比べる build で、`X？` の行が使い回されて落ちる。速報で掛けた V の判定をそのまま使う build で、(d) が落ちる |
| RP-70 | `。` を足しただけの文の訳を使い回してよいこと（3.18 の部分訳の使い回しの品質） | test-designer の席が、`testdata/baseline/pairs/`（seed 0）と `testdata/baseline/pairs_seed1/`（seed 1）の ja の原文のうち末尾が `。` の行だけを取り、そのままの組 a と、末尾の `。` だけを除いた組 b を git の外に作る（行の id と参照訳は同じ）。同じ llama-server（RP-71 の腕 N0）で、`hayamimi-relay eval --pairs <a> --out <ha1>`、`--pairs <b> --out <hb1>`、`--pairs <a> --out <ha2>`、`--pairs <b> --out <hb2>` の順に打ち、`<ev> chrf` で採点し、`cargo test --release -p hayamimi-eval --test rp70 -- --ignored` が 4 組の結果と中継の stdout を読む | b の 2 回を合わせた corpus chrF が、a の 2 回を合わせた値以上。V が両方の訳を止めた行の数が、b の 2 回の合計で a の 2 回の合計以下。b の訳の文末に `.`、`?`、`!` が無く、a の同じ行の訳には在る行の数を記録する（合否に入れない） | 同じ test が、a の 2 回の対から chrF の差と止めた行の差を出す（a 同士の差を記録する） | b の chrF を a より 0.01 低く置いた fixture と、b の止めた行を a より 1 行多く置いた fixture のどちらでも落ちる |
| RP-71 | 推測デコードの llama-server が速く、同じ訳を出す（3.18、8 の R8w） | 3.18 の腕ごとに `hayamimi-llama-pair.service` を起こし直し、`sudo -n nice -n -5 <relay> eval --pairs testdata/baseline/pairs/ --out <腕と回の dir> --llm http://127.0.0.1:18083` を、N0、S3、N0、S3 の順に腕ごとに 2 回打つ（起こす前の memory の確かめと 18081 の止め方は 3.18 の推測デコードの決めの 5）。中継の stdout（`translate.line.ok` と `translate.line.held` の `t_ms`、`e_ms`）と、run ごとの server の log（`journalctl --user -u hayamimi-llama-pair.service` の、その run の間の行）と、`docs/results/r8w/server-args.md` の `--help` と `nvidia-smi` の記録を残し、`cargo test --release -p hayamimi-relay --test rp71 -- --ignored` が読む | S3 の 2 回を合わせた T の 1 回の p50 が、N0 の 2 回を合わせた p50 から、N0 の 2 回の p50 の差の絶対値を引いた値より小さい。E の 1 回の p50 も同じ式で小さい。S3 と N0 で確定の訳が違う行の数が、N0 の 2 回の間で違う行の数以下。S3 の V が両方の訳を止めた行の数が N0 の 2 回の多い方以下で、corpus chrF が N0 の 2 回の少ない方以上。S3 の引数が `--help` の記録に在り、run の間の `memory.used` が 14,000 MiB 以下。腕ごとの p50 と p95 を全部記録する | 同じ test が、N0 の 2 回から、違う行の数と T と E の p50 の差を出す。S3 の各 run の server の log に、下書きが採られたことを示す行（`draft acceptance rate` か `#acc tokens`）が在り、その値が 0 より大きい（下書きが働いた陽性対照）。N0 の run の log にはその行が 0 行である | T の p50 の差を N0 の 2 回の差と同じに置いた fixture、N0 との違う行を N0 同士より 1 行多く置いた fixture、S3 の log の採られた token の数を 0 にした fixture のどれでも落ちる |
| RP-72 | RP-32 の run で port が移った run を無効にする（3.18、8 の R8w） | `cargo test --release -p hayamimi-eval --test rp72` が、fixture の run の dir を RP-32 の判定（`check_dir`）に渡す（`#[ignore]` を持たない） | `rs<N>.events.jsonl` に `type` が `fallback` で `reason` が `in_use` の event が 1 件在る dir は、その run の名前を挙げて `Err`。`rs<N>.events.jsonl` の無い dir も、その名前を挙げて `Err` | 同じ test が、`docs/results/rp32/` と同じ形で `fallback` の無い fixture の dir を、port の理由では落とさない | event を読まない今の判定では、`in_use` の `fallback` を 1 件足した fixture の dir に port の理由の `Err` が出ないので、この test は今の main で落ちる |
| RP-73 | RP-67 の合否が、`t_end`（manifest の `end_sample`）の置き方の誤りで決まっていないこと（3.1 の起点の確かさ、3.17） | `cd native && cargo test --release --jobs 2 -p hayamimi-eval --test rp73` が fixture の行を流す。実走の行は `-- --ignored` を足し、`testdata/lat-ja.wav`、`testdata/lat-en.wav`、`testdata/manifests/lat-ja.json`、`testdata/manifests/lat-en.json` と、RP-67 の実走の行と同じ `docs/results/rp67/` の `e2e1.jsonl` と `e2e2.jsonl` を読む。作る物は 2 つである。1 つ目は `native/hayamimi-eval/src/manifest.rs` の、clip の音声から 3.1 の確か、切れ、不確かを返す関数で、`speech_end` と同じ 320 sample の frame と百分位の式を使い、高さ 35、25、15、10 %、安定の幅 40 ms（動きが 40 ms 以下なら安定。40 ms を含む）、後ろの窓 300 ms（15 frame）、低さの線 6 dB と 10 dB を定数に置く。後ろの窓の「平均」は frame の dB 値の算術平均で、電力の平均ではない。2 つ目は、RP-67 の判定の関数を発話の組で絞って呼ぶ test である。実装の順：RP-73 は、RP-67 が初めて通ったと名乗る前に要る。それより前に、`L` を縮める仕事（H93、VAD、訳の段）から cargo の席と load の窓を取らない | 1. 発話ごとに、組、各高さでの声の終わり、後ろの 300 ms の平均の低さを出す。全発話で `speech_end` を当て直した値が manifest の `end_sample` と一致し、一致しない発話が在ればそれを挙げて `Err`。2. 言語ごとに、RP-67 と同じ数え方の判定（`L` の p95 ≤ 1000 ms）を全発話と確かの発話だけで出し、2 つが同じなら `Ok`、割れたら `Err` で「組で割れた」と言語を名指す。割れの原因を起点と断定しない。確かの発話だけの p95 は、同じ記録でも低さの線で 1,222 ms（10 dB）から 1,531 ms（6 dB）まで揺れ、標本が少ないだけでも割れうるからである。確かの発話に訳の取れた有効な標本が 0 件の言語は `Err`（0 件を一致と読まない）。3. 低さの線を 6 dB と 10 dB の両方で数え、線で判定が変われば `Err`。4. 確か、切れ、不確かの組ごとの `L` の p50 と p95 は、この test の出力にだけ並べて書き、記録だけにする | 組の数が lat-ja で確か 4、切れ 8、不確か 11、lat-en で確か 15（10 dB の線では lat-ja が確か 3、lat-en が確か 12）。git の中の `docs/results/rp32/rs1.jsonl`〜`rs3.jsonl` を合わせて渡すと、ja の `L` の p95 は全発話 1,460 ms（n 291）、確かだけ 1,491 ms（n 60）、10 dB の確かだけ 1,222 ms（n 45）で、判定は割れずに `Ok`（RP-67 の側はどれも `Err`）。`docs/results/e2e/e2e2/e2e2.jsonl` の en は全発話 1,303 ms（n 70）、10 dB の確かだけ 1,271 ms（n 60）で割れない | 確かの発話の標本だけを `L` 900 ms、ほかを 1,500 ms にした fixture（全発話で `Err`、確かだけで `Ok`）が、`Err` で「組で割れた」と ja を名指す。確かの 1 発話の `end_sample` を 300 ms 前へ動かした manifest が、その発話を挙げて `Err`。静かな側から 20 % の高さの尾を 400 ms 持つ合成の clip を確かと読む build（40 ms の安定の検査を外した物）が、その clip の case で落ちる |

## 7. test の名前

各 ID の下の case 行（`T-RP-<n>-nnn`）は test-designer が `rust_port_tests.ja.md` に書く。
この表の test の名前が code に在るかは、機械では当たらない（`hayamimi-eval doc-links` は Markdown の link の行き先の file だけを見て、表の中の名前を読まない）。検証の席が、slice ごとに `rg -n 'fn <名前>' native/` で 1 件在ることを確かめる。
全体の test の数も機械では当たらない。`test result:` の行を足す数え方は、test の stdout の行が割り込んで `test result:` の行が 2 行に分かれる時と、test の中で走る入れ子の cargo の `test result:` の行に弱いからである。数を合否か記録に使う席は、test の binary の数と `test result: ok. N passed` の行の数を並べて貼り、2 つが合わなければ数を使わない。
比べる式は「`Running` の行の数（binary）＋ `Doc-tests` の行の数（crate ごとに 1 本）＝ ok 行の数」で、`rp23` の中で走る入れ子の cargo の `test result:` の行（4 本）は ok 行に数えない。

| ID | 受入 test の名前 | 置き場 | 陰性の case |
|---|---|---|---|
| RP-1 | `hayamimi_scorecard_not_worse_than_python` | `native/hayamimi-eval/tests/rp01.rs` | ja の clip を SenseVoice の tier へ送る build |
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
| RP-74 | `rp74_split_estimate_beats_cpu_int8_in_every_condition` | `native/hayamimi-eval/tests/rp74.rs` | `encoder_only` を同じ組の A の値に置き換えた fixture、足し算の式が 15% を外れる fixture |
| RP-75 | `rp75_split_recognizer_matches_the_cpu_reference_of_the_same_files` | `native/hayamimi-asr/tests/rp75.rs` | 参照の text を別の clip の順に並べ替えた fixture |
| RP-76 | `rp76_split_recognizer_ja_cer_not_worse_than_cpu_int8` | `native/hayamimi-eval/tests/rp76.rs` | ja の clip を SenseVoice の tier へ送る build |
| RP-77 | `rp77_recognizer_without_cuda_falls_back_to_cpu_int8_and_says_so` | `native/hayamimi/tests/rp77.rs` | GPU の在る run を落ちた run として渡す、`gpu.fallback.cpu` を消した fixture、patch を当てない library に繋いだ split の run |
| RP-78 | `rp78_split_recognizer_shortens_l_asr_without_slowing_translation` | `native/hayamimi-eval/tests/rp78.rs` | encoder が CPU で作られた split の run |
| RP-24 | `rp24_windows_and_wsl_builds_match` | `native/xtask/tests/rp24.rs` | ORT の版を変えた build |
| RP-25 | `rp25_translation_flicker_is_bounded` | `native/hayamimi-eval/tests/rp25.rs` | 先の訳を外へ送る偽の中継 |
| RP-26 | `rp26_messages_follow_the_locale` | `native/hayamimi/tests/rp26.rs` | 文言を直書きした build |
| RP-27 | `rp27_string_stages_do_not_import_ort_or_cuda` | `native/hayamimi-asr/tests/rp27.rs` | `use ort` を足した `itn.rs` |
| RP-28 | `rp28_cpu_fallback_latency_is_recorded_apart` | `native/hayamimi-eval/tests/rp28.rs` | CPU の run として渡した GPU の run |
| RP-29 | `rp29_missing_resources_fall_back_and_say_so` | `native/hayamimi/tests/rp29.rs` | 外したのに event を出さない build、在るのに落ちる build |
| RP-30 | `rp30_fallback_stage_and_reason_lists_match_the_code` | `native/hayamimi/tests/rp30.rs` | 1 つ消した一覧、code に無い値を足した一覧 |
| RP-31 | `rp31_latency_counts_only_shown_translations_of_their_own_utterance` | `native/hayamimi-eval/tests/rp31.rs` | `held` の空の訳を先に置いた列、遅れて着いた `final` |
| RP-32 | `rp32_alternated_runs_under_the_same_load_beat_python`（実走の行 T-RP-32-029 の test。名前は code に在る物のまま変えない）。有効の線を行ごとに選ぶ形の受入は `rp32_validity_line_follows_the_rule_that_ended_each_final`。ほかの行の test の名前は T の 4.42 の表のとおり | `native/hayamimi-eval/tests/rp32.rs` | load の記録が無い run、load 2 以上の Rust の run、`pairs.txt` の無い 2 対。`silence` の標本に早い線を当てる判定、`asr.endpoint.decided` に結べない `final` を既定の線で数える判定 |
| RP-33 | `rp33_first_translation_after_a_failed_draft_beats_python` | `native/hayamimi-eval/tests/rp33.rs` | 落ちた行を 600 ms 遅らせる偽の中継 |
| RP-34 | `rp34_wait_before_final_decode_not_longer_than_python` | `native/hayamimi-eval/tests/rp34.rs` | 確定の前に 300 ms 待つ偽の engine、GPU の記録が無い run |
| RP-35 | `rp35_latency_work_keeps_chrf_and_verify` | `native/hayamimi-eval/tests/rp35.rs` | V を掛けずに出す偽の中継、T の訳をそのまま確定にする偽の中継 |
| RP-36 | `rp36_ko_error_within_the_shipped_build_cap` | `native/hayamimi-eval/tests/rp36.rs` | ko の平均を 86940 にした記録 |
| RP-37 | `rp37_hedge_mismatch_is_logged_in_zh_ko_es_and_held_in_ja_en` | `native/hayamimi-relay/tests/rp37.rs` | zh で推量の不一致を止める build、ja で止めない build、zh で否定と数を止めない build |
| RP-38 | `rp38_eval_logs_one_hedge_event_per_line` | `native/hayamimi-relay/tests/rp38.rs` | `settle_other` で V を 2 回掛けた build |
| RP-39 | `rp39_number_scales_read_in_zh_ko_es` | `native/hayamimi-relay/tests/rp39.rs` | 数を変えた訳を止めない build、倍の語を読まない今の `numbers.rs` |
| RP-40 | `rp40_pages_and_relay_events_reach_from_the_lan` | `native/hayamimi/tests/rp40.rs` | `--serve-host` を付けない run |
| RP-41 | `rp41_stream_listen_reads_events_over_sse` | `native/hayamimi-eval/tests/rp41.rs` | WebSocket で繋ぐ今の `listen` |
| RP-42 | `rp42_held_line_logs_both_reasons_without_the_text` | `native/hayamimi-relay/tests/rp42.rs` | T の側の理由を載せない build、本文を載せる build |
| RP-43 | `rp43_nasasou_is_read_as_negation` | `native/hayamimi-relay/tests/rp43.rs` | 「なさ」だけを足した build、表を足さない build |
| RP-44 | `rp44_new_heldout_set_is_fresh_and_has_nasasou` | `native/hayamimi-eval/tests/rp44.rs` | dev の id を混ぜた ids |
| RP-45 | `rp45_edit_template_never_reaches_the_subtitle` | `native/hayamimi-relay/tests/rp45.rs` | `translation_of` を外した build、札を行の途中でも読む build |
| RP-46 | `rp46_english_recognizer_loads_before_ready` | `native/hayamimi/tests/rp46.rs` | 最初の英語の発話で読み込む形 |
| RP-47 | `rp47_next_draft_is_not_queued_behind_the_previous_edit` | `native/hayamimi-relay/tests/rp47.rs` | 直列の訳の lane |
| RP-48 | `rp48_hedge_mismatch_into_english_is_logged_and_into_japanese_held` | `native/hayamimi-relay/tests/rp48.rs` | ja→en で推量を止める今の build、en→ja で原文の推量を落とした訳を止めない build、ja→en で否定と数を止めない build |
| RP-49 | `rp49_spelled_clock_time_is_read_as_hour_and_minute` | `native/hayamimi-relay/tests/rp49.rs` | 時刻を読まない今の `numbers.rs`、単位の語の次に十の位の語を一般に続けて読む build |
| RP-63 | `rp63_into_japanese_a_dropped_hedge_is_held_and_an_added_hedge_is_logged` | `native/hayamimi-relay/tests/rp63.rs` | 訳だけの推量も止める build、原文の推量を落とした訳を止めない build、8 つを足さない build、`ならなかった` を単独で除く build |
| RP-64 | 置かない（R8u を作らない） | なし | なし |
| RP-65 | `rp65_a_one_or_two_letter_final_outside_the_session_script_is_dropped` | `native/hayamimi/tests/rp65.rs` | 字の数だけで捨てる build、捨てない build |
| RP-66 | `rp66_early_end_shortens_the_wait_without_splitting_utterances` | `native/hayamimi-eval/tests/rp66.rs` | `--endpoint silence` の run、完結の判定を常に真にした build の run |
| RP-67 | `rp67_speech_end_to_translation_p95_is_within_one_second` | `native/hayamimi-eval/tests/rp67.rs` | 今の main の E2E の記録、許す数より 1 件多く 1000 ms を超える fixture |
| RP-68 | `rp68_flush_inside_speech_aborts_and_the_early_end_never_flushes` | `native/hayamimi-asr/tests/rp68.rs` | 確定の時に A へ `flush` を打つ build、`finish` の後に detector を作り直さない build |
| RP-69 | `rp69_a_final_that_only_closes_the_last_partial_reuses_its_translation` | `native/hayamimi-relay/tests/rp69.rs` | `final` で速報の依頼を止める今の形、句読点を全部除いて比べる build、速報の V の判定をそのまま使う build |
| RP-70 | `rp70_translating_without_the_closing_mark_keeps_chrf_and_verify` | `native/hayamimi-eval/tests/rp70.rs` | b の chrF を 0.01 下げた fixture、b の止めた行を 1 行足した fixture |
| RP-71 | `rp71_speculative_server_is_faster_and_translates_the_same` | `native/hayamimi-relay/tests/rp71.rs` | T の p50 を N0 と同じにした fixture、違う行を 1 行足した fixture |
| RP-72 | `rp72_a_run_whose_server_moved_its_port_is_refused` | `native/hayamimi-eval/tests/rp72.rs` | event を読まない今の RP-32 の判定 |
| RP-73 | `rp73_the_l_verdict_is_the_same_on_utterances_whose_end_is_certain` | `native/hayamimi-eval/tests/rp73.rs` | 確かの発話だけが 1000 ms 以下の fixture、`end_sample` を 300 ms 前へ動かした manifest |
| RP-55 | `reset_parts_forget_the_previous_sessions_audio_language_and_speakers` | `native/hayamimi-asr/src/pipeline.rs` の `mod tests` | `History::clear` か `Router::reset_session` を呼ばない `reset_parts` |
| RP-56 | `final_event_switched_true_when_language_changes` | `native/hayamimi/src/session.rs` の `mod tests` | `switched` を常に偽で出す `wire.rs` |
| RP-57 | `der_breakdown_isolates_confusion_from_miss` | `native/hayamimi-eval/src/der.rs` の `mod tests` | Confusion を Miss に足す内訳 |
| RP-58 | `windows_com_scope_uses_existing_changed_mode_apartment` | `native/hayamimi/src/capture/windows.rs` の `mod tests`（純関数は cfg を外した module に置く） | `RPC_E_CHANGED_MODE` を失敗として返す形 |
| RP-59 | `reconstruct_inserts_question_mark` | `native/hayamimi-asr/src/punct/four.rs` の `mod tests` | 4 クラスを既定にした build |
| RP-60 | `two_speaker_clip_is_cut_by_the_rule_from_ami` | `native/hayamimi-eval/src/two_speakers.rs` の `mod tests` | 重なりの在る窓だけの RTTM |
| RP-61 | `rp61_live_refine_does_not_delay_the_subtitle` | `native/hayamimi-eval/tests/rp61.rs` | 清書を live と同じ `Router` で解く build |
| RP-62 | `pool_audio_for_group_uses_diarizer_turns_when_present` | `native/hayamimi-asr/src/recluster.rs` の `mod tests` | turn を見ずに buffer の全部を返す関数 |
| RP-19（R12a1） | `rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks` | `native/hayamimi-eval/tests/rp19.rs` | 一覧に在る名前が list にも在る（消し忘れ）、一覧の数が `ALLOWED_NOT_YET` を超える |

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
| R3 | 言語の振り分け（whisper-tiny と SenseVoice の二重確認、粘り、5 つの tier、Omnilingual、LRU、清書、second opinion、ko の分かち書き）。言語判定の CPU への落とし、GPU の memory の上限での LRU と CPU への落とし、振り分けの認識器と言語判定の model が無いときの落とし（3.15）。R3a（振り分け）と R3b（認識器、常駐、`hayamimi-eval scorecard`、`lat-multi`）の 2 席に分ける（「07:00 の計画」）。言語判定を `hayamimi` の実行 file に繋ぐ（`Engine::open` で `lid::open` を呼び、本物の run で言語判定の event を出す）のは R3b である。session を作る所の process 全体の lock（3.4 の「session は 1 本ずつ作る」）も R3b が `hayamimi-gpu` に置き、R2 の句読点と R4 の話者の session もその lock を通す。話者の session を lock に通すのは、R3b と R4 のうち後に main へ merge する側である。lock の受入は T-RP-16-036 で、lock の無い形の abort（9 回で 2 回）は再現の稀な物なので T-RP-16-037 は記録だけにする（lock を外した 3 つの構成の 20 回ずつで 0 回だったことも記録に書き、合否に使わない）。RP-1 の合否を決める run は、Windows の配る形の build（6 の RP-1）で打つ。その build で振り分けを通すと、zh と yue は Python と同じ値になる（10 の H35。R3c の検証の席の可、`b8fd07e`）。R3b の merge の後に、書いていない検証の席が RP-10 を `lang` の条件つきで Windows で打ち直す（8 の R5 の行）。R3b の branch が 3.13 に足した 6 つの event（`asr.model.loaded`、`asr.model.evicted`、`asr.lang.unknown`、`gpu.memory.unreadable`、`model.fallback.substitute`、`model.fallback.chunks`）は、名前の形（`<domain>.<action>.<outcome>`）と 3.15 の落とし方に合うので採る | RP-1、RP-22、RP-16 の言語判定の行、RP-29 の R3 の行 | 同上 |
| R4 | 話者（CAM++、pyannote、FastClustering、全体の対応づけ、仮の印）。FastClustering と全体の対応づけの CPU の p50 を `hayamimi-eval stages` で測り、`gpu_stages.md` の「清書の clustering」の CPU の行に置く。話者の model が無いときは印なしで動かす（3.15）。確定文は話者の埋め込みを待たずに `speaker` を空で出し、埋め込みの後に `{"type": "speaker", "id": <確定文の id>, "speaker": <印>}` を同じ `id` へ 1 回出す（字幕を出す thread に埋め込みの約 70 ms を載せないため）。印の決まらない発話には出さない。確定文が話者を待たないことは、遅れの差ではなく、同じ `id` の `asr.final.published` が `speaker.embedding.started` より前か同じ時刻であることで確かめる（3.13。遅れの差は run ごとに約 40 ms 揺れ、待つ形と待たない形を見分けられない） | RP-15、RP-29 の R4 の行（T-RP-29-027、028）、T-RP-15-012 と 013。RP-8 の根拠 4 の式（清書の clustering の CPU の p50 が GPU の往復の下限より小さい） | 同上 |
| R5 | 取り込み（wasapi、cpal、mix、`/ingest`）と server の全 route。速報（伸びていく音声を約 0.5 秒ごとに解き直し、`partial` の event を出す。2.2）。`<ev> stream --sink loopback`（wav を既定の出力の装置へ鳴らし、送った sample の時刻を記録する）。取り込みから認識器へ渡すまでと、確定文の event を `/events` へ書き出すまでの CPU の p50 を `hayamimi-eval stages` で測り、`gpu_stages.md` の 2 段の CPU の行に置く。取り込みの装置が無いときと、全 route の port が使えないときの落とし（3.15）。確定文の `source`（会話字幕の設計 3.3）は `--input` から決め、`wav` は `file`、`mic` と `mix` と `ws` は `mic`、`loopback` は `loopback` とする。`--source <mic|loopback|file>` で上書きでき、`/ingest` で loopback の音を受ける process（E2E-2 の陽性対照の process B）は `--source loopback` を付ける | RP-10（`lang` が `en` の条件を除く。英語の経路は R3b が `--lang en` と振り分けを実行 file に繋ぐ時に入り、R3b の merge の時に RP-10 を `lang` の条件つきで打ち直す。R3b は W3 の終わりまでに merge する。E2E-2 が英語の経路を要るため）、RP-11、RP-12、RP-29 の R5 の行。RP-8 の根拠 4 の式（取り込みと配信の CPU の p50 が GPU の往復の下限より小さい） | 同上 |
| R6 | Dioxus の 3 頁（designer の板が先）。`hayamimi` の server が `/`、`/dashboard`、`/transcript` で `hayamimi-ui` の `web/index.html` を返し、`/ui/*` で wasm の出力を配る結線は、R5 と R6 のうち後に main へ merge する側が R5 の route に足す | RP-12 の頁の行と、overlay の同一性の test。結線の受入は、本物の `<hy> --wav tests/golden/ja/ja_014.wav --serve 0` を起こし、`/`、`/dashboard`、`/transcript` の GET が `web/index.html` と同じ bytes を返し、その index.html が読む `/ui/` の下の wasm と js の GET が全部 200 であること（陰性対照は、`/ui/` の下に無い名前の GET が 404） | crate を消す |
| R7 | 認識の遅れの詰め。R1 で遅いと分かった段を直す。3.5 の CPU の時間は 1 stream で測った値で、mic と loopback の 2 音源同時の CPU は未測定なので、2 音源を同時に流した CPU の段の p50 と p95 を測る。認識は 2 音源同時で GPU（fp32）と CPU（同じ fp32 と int8）を測り直し、CPU の側が GPU より遅くなれば 3.5 の置き場を評議へ戻す。R7 の 1 回目の値（peers 1 の認識全体の p50 が GPU fp32 360316 µs、CPU fp32 487816 µs、CPU int8 396586 µs）は load 10〜32 の下で、CPU と CUDA を続けて取っていないので、置き場の判断に使わない（NOT VERIFIED）。測り直しは W4 の初めの窓（2026-10-15 04:45〜04:55）で済み、3 回の対のどれでも CPU int8 が GPU fp32 より速かったので、3.5 の置き場は変えず評議へも戻さない（3.5 の表の後の段落）。2 回目以降の run の前の load が 2 を超えていたことは、結論の向きを変えないので置き場の判断に使い、値は NOT VERIFIED とする。H31 は置き場に効かないので調べない（CPU が速ければ認識は GPU に載らない）。`compare.rs` のうち、Python と Rust の run の `L` と `L_asr` を比べる部分は R8e の RP-3 の道具を正とし、R7 は段の時間の比べだけを持つ。重なった関数は、後に main へ merge する側が片方に寄せる。H9（`--min-silence` 0.25）と H10（確定の解き直しを省く）は `L_asr` を縮める物で、R8h の後の席が E2E の後に打つ（10 の H9、H10）。H41 の同じ process での順の確かめも、同じ席が E2E の後に打つ | RP-4、RP-7 | 設定と commit を revert |
| R8a | 中継が Rust の `hayamimi` の `/events` を購読する（3.3 の 5 項目と `src_sha256`）。5 項目を `hayamimi` の確定文の event に足す。`<ev> stream` が中継の SSE の訳も読む | 会話字幕の CS-21、CS-22 | 中継の購読先を戻す |
| R8b | llama-server を Windows の native に置く（`%LOCALAPPDATA%\hayamimi\models\llm\`、slot 4）。起こす側が 3.11 の確かめを打ち、GPU が無ければ `-ngl 0` か CPU の build で起こし、`gpu.fallback.cpu`（`stage` が `translate`）を出す。llama-server に届かないときは訳を保留して原文を出し続ける（3.15） | 会話字幕の CS-20、RP-16 の訳の行、RP-29 の R8b の行。Windows の CUDA 13.4 の llama-server で T を測り直し、`gpu_stages.md` の T の行を置き換える（RP-23 の T の行の形） | WSL の llama-server に戻す |
| R8c | T の訳を先に出し、E の訳で置き換え、確定の訳だけを外へ出す。外へ出す口は、確定の訳（`settled: true`）だけを受ける送り先の trait 1 つにし、Zoom、Teams、SRT の中身は R8g が作る。外の送り先に届かないときは SRT と重ね窓に残す（3.15）。会話字幕の T-CX-1-002 は test-designer（sonnet）の席を R8c と同じ時に出して書き替え、R8c の検証の前に入れる（V に渡る文字列は E の応答で、V は T の応答と E の応答に 1 回ずつ、多くて 2 回）。wire の文書（会話字幕の設計 3.3）と実装の差は、R8c が差の一覧を返し、設計の席が同じ波の検証の前に 3.3 を直す | RP-25、会話字幕の CS-5、CS-25、RP-29 の R8c の行 | 中継の commit を revert |
| R8d | 速報の訳（re-translation、最長共通接頭辞、打ち切り、`cache_prompt`）。確定した字形で描いた接頭辞を後で消さないこと（RP-25 の 3 つ目の条件）を中継で守るのも R8d で、R8d はそれが未だでも merge してよく、R8d の続きの席が W3 の R8e の計測より前に入れる。R8e は RP-3 の run でその回数を測る（T-RP-25-010）。RP-5 の対象の発話（6 の RP-5 の陽性対照）に合わせ、test-designer の席が T を直す（セラが運ぶ）。T-RP-5-007 の対象外を「`final` の無い発話」に替え、`partial` だけが在って `final` の無い発話を 1 つ含む fixture で、その発話が p95 に入らず、別の数として 1 と数えられる行を足す | RP-5、RP-25 | 設定で速報の訳を切る |
| R8e | 4 つの slot と負荷の下の `L`。CPU に落ちた run の `L` を測り、RP-28 の仮の上限を置き直す | RP-3、RP-28 | slot の数を戻す |
| R8f | `--translate` の言語を llama-server の T で訳す。中継の `eval`、chrF の Rust の実装と `hayamimi-eval chrf`（sacrebleu 2.6.0 と同じ値）も R8f が持つ。`hayamimi-relay live --translate` に ja と en の外の言語（zh、ko、es ほか）を出す経路（`Translation.lang` を ja と en の 2 つに限らない形）も R8f の受入に入れ、W3 の終わりまでに入れる（E2E は ja と en だけなので止めない）。表に無い言語は 3.15 の `translate.fallback.skipped` で落とす。zh、ko、es の V は形、数、ラテン文字の名前だけを見る（10 の H36） | RP-13、RP-14。T の case 行は T-RP-13-010〜015 で足りる。加えて、R8f の検証の前に test-designer が 2 行を足す。1 行目は 014 の対で、表に在る言語だけの pairs では `translate.fallback.skipped` が 0 件。2 行目は live の経路で、偽の llama-server に `--translate zh` で 1 行流し、`lang` が `zh` の `translation` が 1 件。live の test は `parse_args` の返す設定を、`cli::run` が使うのと同じ「中継を起こして handle を返す関数」に渡して起こし、bind した port の `/events` を読み、handle で止める。`cli::run` はその関数を呼んで待つだけの薄い関数にし、走り続ける `cli::run` を test から直に起こさない。この形に合わせて、test-designer の席が T に 2 行を足す（セラが運ぶ）。017 に「`lang` が `en` の `translation` が 0 件」を足すことと、live で表に無い言語を渡す行（`--translate xx,zh` で `translate.fallback.skipped` が `xx` に 1 件、`/events` の `fallback`（`to` が `skipped`）が 1 件、zh の訳は出続ける）である | commit を revert |
| R8g | 外の送り先の中身。R8c の送り先の trait に、Zoom の字幕 API、Teams の CART、SRT の書き出し（会話字幕の設計 3.10 と 3.13）を足す。指定は中継の `hayamimi-relay live --send <send.toml>`（Zoom の token と Teams の link）と `--srt <path>` で、どちらも無ければ送らずに動く。token か link が手元に無い機械では CS-17 と CS-18 を打たずに残る穴にする（state.json の `blocked_on` と同じ）。動画から SRT を作る `<relay> srt <mp4>`（会話字幕の設計 3.13）と、`send.toml` の権限の検査と token を log に出さないこと（CS-28）も R8g が作る。どれも E2E には要らない（E2E は mic と loopback の 2 本で、送り先と動画を使わない）。YouTube Live へは送らない（会話字幕の設計 1 の「作らない物」と 4。配信は OBS の字幕頁の焼き込みで足りる）。`send.toml` の形と送り方は会話字幕の設計 3.10、`srt` の手順は 3.13 に従う。`srt` は `hayamimi --input ws --source file --ws-port <空き port>` を起こし（`/ingest` の port は `--ws-port`。`--serve` は `/events` の HTTP の port で、`srt` は使わない）、`/ingest` に繋いでから作業 file の音声を送り、同じ接続で返る event から `final` を集める（3.1 の測りと同じ口）。`hayamimi --wav` を起こしてから `/events` に繋ぐ形を採らないのは、`/events` が過去の `final` を遅れた client に送り直さず、繋ぐ前の `final` が落ちるからである | 会話字幕の CS-14、CS-15、CS-17、CS-18（token か link が在るときだけ）、CS-28、RP-29 の R8c の行（送り先に届かないとき） | commit を revert |
| R8h | 主指標 `L` の差の詰め（3.16）。1：`stream` の集計の数え方（`held` と空の訳を数えない、`id` と `t1_s` で結ぶ）。2：T が V に落ちた行の最初の訳。B（R8f の V の直し）だけを進め、A は作らない（3.16 の 2）。3：復号の順は変えず、確定の復号の前の待ちを `L_asr − decode_ms` で測る。待ちの log と、run の前後の `nvidia-smi` の記録を 4 の前に入れる。4：load の記録つきで固定した Python の基準と、同じ規則で測った Rust の run を比べる | RP-31、RP-32、RP-33、RP-34、RP-35 | commit を revert。1 を revert すると `L` の数え方が前の形に戻る |
| R8i | zh、ko、es の V で、推量の不一致を止める理由から外し、`translate.hedge.logged` の log にだけ残す（3.6）。否定、数、形、名前の検査と、ja と en の推量の検査は今のまま。merge の前に dev の seed 0 と seed 1 で vprobe を打つ。続きの席が `eval.rs` の `verdict` を、確定の訳に掛けた V の時だけ log を出す形に直す（下書きの V では出さない） | RP-37、RP-38 | その 1 commit を `git revert` |
| R5b | 字幕の server（`native/hayamimi/src/run.rs` の bind）と中継の SSE（`native/hayamimi-relay/src/hub.rs` の bind）に `--serve-host <addr>` を足す。既定は `127.0.0.1` のまま（Python の `subtitle_server.py` と `--ws-host` と同じ）で、LAN の端末から頁を見る時は `--serve-host 0.0.0.0` を付ける。既定を LAN にしないのは、`/config` と `/reset` が認証なしで設定と状態を変えるので、LAN へ開くのは利用者が選んだ時だけにするためである。`--serve-host` が開くのは `--serve` の router だけで、`/ingest` は `--ws-host`（`native/hayamimi/src/inputs.rs` の `ws_host`、既定 `127.0.0.1`）で別に決まり、LAN に開かない。loopback でない address に bind した時は、起動の stderr に LAN の URL を 1 行出す。手順書（`docs/guide/run_hayamimi.ja.md`）の E2E と LAN で見る手順に `--serve-host 0.0.0.0` を書く | RP-40 | commit を revert |
| R8m | `hayamimi-eval` の `ingest.rs` の `listen` を、`/events` の SSE を読む形に直す（Rust と Python の `/events` はどちらも SSE） | RP-41 | commit を revert |
| R8n | 6 と E2E の表に名前が在って repo に無い受入 test を作る：RP-32 の判定（`rp32_alternated_runs_under_the_same_load_beat_python`）、RP-33、RP-34、RP-35、E2E-1、E2E-2 | RP-32〜35、E2E-1、E2E-2 の test が在り、記録（`docs/results/r8h_verify/`）を読んで値を出す | commit を revert |
| R8p | 最優先。E の答えから雛形の札（`English:`、`Draft Japanese:`、`Rewritten Japanese:` と ja→en の同じ形）を剥がし、札の後ろの訳だけを V に渡す。空になる答えだけを `form` で止める。ja へ向けた V の形の検査を「英字や改行を含む文を落とす」形に厳しくはしない。ラテン文字の名前や英字の略語を含む正しい訳まで止まり、H53 の訳の取れた割合が下がるからである（10 の H54）。剥がす処理は `native/hayamimi-relay/src/edit_answer.rs` の `translation_of` に在り、`pipeline.rs` の `edit` と `eval.rs` の E の答えから呼ぶ。札は行頭の `Draft` 系と `Rewritten` に、訳の言語名（綴りゆれは 2 文字まで）と `:` が続く形で、最後の札の後ろが訳になる。行の途中の語、言語名でない語を挟む形、単独の `English:` は札にしない。札を答えの行頭の札の並び（原文、下書き、書き直しの順の組）だけと読む形は採らない。記録の en→ja の雛形 20 件のうち 12 件は原文の札 `English:` を持たず、組を求めると剥がせないからである（10 の H60 の数え）。綴りゆれの 2 文字の許しは、言語名から 2 文字以内の実在の別の語（`Javanese`、`Enlist`）を行頭の札の形で書いた訳の行も札と読む。この代償は受ける（10 の H60）。`</s>` を含む答えは切らずに V へ渡す | RP-45 | commit を revert |
| R8q | en→ja の session（`--lang en` と、振り分けで en を使う設定）を起こす時に、英語の認識器 v3 を `ready` の前に読む（`hayamimi-asr` の `pipeline.rs` の `open_route` が、方針が英語に固定されているか言語を判定する時に `Router::prefetch` で読む。`--lang ja` など英語以外に固定した session は読まない）。`/ingest` は `session_start` が出るまで `ready` を返さない（`session_start` は、モデルを読み終えて `asr.model.loaded` を log に出した後に出る。`/events` の client が `session_start` を待つのと同じ）。読み込みに失敗しても止めず、最初の英語の発話を答える tier が読み、`model.fallback.substitute` を 1 回出す。`final` の `decode_ms` から読み込みの時間を外し（`Router::transcribe` が `Decode::take_load_ms` の分を引く。英語に限らず、どの tier の初回の読み込みも入れない）、`decode.rs` の doc comment（読み込みは入れない）に code を合わせる（RP-34 の `L_asr − decode_ms` が待ちを正しく測るため）。v3 は CPU の provider で作るので、GPU の memory（RP-22 の 14,000 MiB）と、同じ process の CUDA の session の組（H41）には触れない。増えるのは host の memory だけである。en の run で約 0.93 GB、既定の振り分けの run も v3 を読むので同じだけ増え、起動から open までが約 5〜7 s 延びる。ja の run は GPU も常駐量も変わらない） | RP-46 | commit を revert |
| R8r | 訳の lane で、新しい確定文の T を前の行の E の後ろに並べない（slot 4 の範囲で、T を E より先に頼む） | RP-47 | commit を revert |
| R8s | 評議の条件つき GO。1 と 2 は別々の commit にし、片方だけを `git revert` で戻せる形にする。1（H53 の 3）：ja→en（訳の向きが en）の推量の不一致を止める理由から外し、`translate.hedge.logged` の log にだけ残す（RP-37 の zh、ko、es と同じ形）。en→ja の推量は、原文の推量が訳で落ちた時だけ止め、訳だけが推量を持つ時は止めずに `translate.hedge.logged` を 1 件出す（8 の R8t）。否定、数、形、名前の検査は両方向で今のまま。入れる理由は 2 つである。H53 の入口の条件（`uncertainty` が主因）は満たしていない（抜けた 13 件の内訳は number 5、uncertainty 4、negation 4）。それでも、人手の参照訳でも en の推量の検査が 50 行中 3 行を止めており（`docs/results/rp13_v_diagnosis.ja.md:31-34` の en の行）、V は正しい訳を推量で止めている。速さが第一、品質が第二なので、正しい訳を E の待ちと訳の欠けへ回す止まりを外し、否定と数の止まりは残す。2（時刻）：`At one thirty` が今の main で止まるのは、`one thirty` を 1 と 30 と読むのに、原文の `1時半` には 30 が無いからである（実装の前に再現した）。ja と en の間の V の数の検査（両方向）で、ja の `N時半` と en の `half past N` に 30 を 1 つ足して読む（`spelled.rs` の `half_hour_values`、`verify.rs` の `numbers_agree`）。en の側で時と分を組として読む形は採らない。半時の語に 30 を足せば両側の数の並びが揃い、`one thirty` の読みを変えずに済むからである。単位の語の次に十の位の語を一般に続けて読む形も採らない（時刻でない所の数の読みが変わり、数の壊した行の止まり 33/33 が動くおそれがある）。`two thirty` を `1時半` の訳にした食い違いは止まる対照を置く。3（観測）：E2E-1 の打ち直しの中継に `--verify-text /home/fukugan/.cache/hayamimi-e2e-20261015/e2e1.verify.txt` を付ける。出力は ext4 の repo の外に置き、git に入れない | RP-48、RP-49。1 は merge の前に dev の seed 0 と seed 1 で `vprobe --lang en` を打つ（RP-48）。RP-37 の行と陰性対照（今の「ja と en は止める」）の書き直しは test-designer の席が別に書く | 1 と 2 の commit をそれぞれ `git revert` |
| R8t | 評議の条件つき GO。1（推量）：en→ja の推量の検査を片向きにする。原文の推量が訳で落ちた時だけ `uncertainty` で止め、訳だけが推量を持つ時は止めずに `translate.hedge.logged` を 1 件出す。原文の推量を落とした訳は、原文が言い切らない事を言い切って字幕に出すので止め続ける。訳だけが足した推量は意味を弱めるだけで、止めると正しい訳を E の待ちと訳の欠けへ回すので、速さを先に取る。採らない手段は 2 つである。`would` を en の推量の表に足す形は採らない（`I would like to order.` のような推量でない依頼を推量と読み、その訳を止める）。「だろう」「でしょう」だけを log に回す形も採らない（`could`→`かもしれません` のように、他の推量の語を訳が足した行が止まり続ける）。2（否定）：ja の義務の過去形と「なくては」の形が否定と読まれる。`exclude` は長い順に原文から除くので、`なければならなかった` は `なければなら` を除いた後に `なかった` が残り、否定の表の `なかっ` に当たって `negation` で止まる（`ないといけなかった` も `ないといけ` を除いた後に同じ）。`なくては〜` の形は除く語が無く、`なく` に当たる。RP-63 の 8 つの完全な形を `exclude` に足す。採らない手段は 3 つである。語幹（`なくては`）だけを除く形は、除いた後に `ならない` の `ない` が残り、止まりが直らない。`なくちゃ`、`なきゃ` は記録に出ていない口語の縮約で、出た形だけを足す。`ならなかった` を単独で除く形は、本当の否定（`雨にならなかった`）まで否定でなくする。D はこの行を先に main に置き、code と test は実装の席が `feat/r8t` に置く | RP-63 | commit を revert |
| R8u | 作らない。T と E の両方が `number` で止まった行に、V が原文から読んだ数を名指す 1 文を足して訳し直す形は、probe で数を作る行の多くを直さなかった（10 の H66 の表）。その行は held のまま訳を出さず、原文の字幕を出し続ける。採らない手段は 4 つである。訳し直し（H66 の表のとおり効かず、効いた形は訳の言語が崩れる）、V を緩めて通す形（原文に無い数が字幕と外の送り先に出る）、QE の再ランクと MBR（候補の数だけ遅れが延びる）、量子化を変える形（Q8_0 のまま据え置く） | なし（RP-64 は欠番） | 作る物が無い |
| R8v | 発話の終わりの判定を 2 段にする（3.17）。**手番 1（probe、流し直し）**：`origin/feat/r8v-probe` の `f925f8c` の `native/hayamimi-asr/tests/r8v_probe.rs` を次の形に直して流し直し、`docs/results/r8v/probe.md` を書き替える。(1) 候補が「発話の中」かは manifest の `end_sample` で決めず、同じ A の segment の中で、候補の後に声が続くか（候補の後、その A の segment の終わりまでに B が声の始まりを読むか、A の segment が候補の後ろへ声の区間を持つか）で決める。(2) A の segment の範囲（始まりと終わりの sample）と、A が 1 発話を割った数を発話ごとに出す。(3) 割れを「B だけ」（A は 1 つの segment なのに、完結で確定した候補で 2 つ以上の final になる）と「A も」（A が既に 2 つ以上に割っている）に分けて数える。(4) 遅れは、完結の候補が一度も出ない発話（今の結果で ja の 5/23）を A の検出の時刻に落とし、全発話で p50 と p95 を nearest-rank で取る。(5) engine は候補ごとに開き直す形のままにし、1 つの engine の detector を `finish` の後に使い回さない（RP-68 が通るまで）。手番 1 の結果は `origin/feat/r8v-probe` の `473c879` の `docs/results/r8v/probe.md` と `heldout.md` に在る。「B だけが作る割れが 0」の線は、割れが本物の文の切れ目か、文の途中か、尾の雑音かを区別しないので使わず、通す線は RP-66 の文の途中の切断の率に替える。`vad_ms` は A と B を合わせた p95 が 2 ms 以下のまま。**手番 1b（仕分け、新しい音声は流さない）**：`r8v_probe.rs` の 430〜437 行の remainder（確定の線の後ろの文）の文面も印字するように直し、今ある B だけの割れ（lat の 6 件、held-out の ja の 9 件、AMI の 31 件。今の生の出力は `/home/fukugan/.cache/hayamimi-e2e-20261015/r8v/` の `lat2.txt`、`held-ja.txt`、`held-en.txt`、`kept.ids`）を、3.17 の「RP-66 と RP-67 の数え方」の 8 と 9 で仕分け、3 つの数と ja の 2 通りの仕分けの一致の数を `docs/results/r8v/split-kinds.md` に書く。割れた標本の訳の chrF は手番 1c では取らず、RP-66 の run（手番 3）で記録だけにする。遅れは数え方の 10 の最後の final で取り直す。評議が、RP-66 の文の途中の切断の率の上限（RP-66 の行）と数え方の 9 の一致の上限を、手番 1c の前に固定した。手番 1c は B の `min_silence` 0.12 秒だけを流す。0.12 秒が上限を超えたら、手番 2 へ進まず評議へ返す。**手番 1c（新しい held-out、1 回だけ流す）**：ja は ReazonSpeech の test（Hugging Face の `japanese-asr/ja_asr.reazonspeech_test`、5,263 行、参照の文に `。` と `、` が在る。S32）の行のうち、`testdata/eval_real` の ja の 15 本の参照の文と同じ文の行を除き、長さ 2 秒以上 15 秒以下の行を行の番号の順に先頭から 80 本取り、`hayamimi-eval fetch-set --concat` で 1.5 秒の無音を挟んでつなぐ。行の番号と音声の sha256 を manifest に書く（sha256 は記録で、取得の条件にしない）。`testdata/fleurs_bench/ja` は 100 本で `id` が 38 種しか無く、手番 1 の held-out で使い切ったので使わない。en は `testdata/eval_diar` の AMI の 5 会議で、手番 1 と同じ turn の規則（1 人の turn で 1 秒以上、前後 0.3 秒に他の話者が掛からない）に当たる turn のうち、手番 1 で取った各会議の先頭の 12 本（`kept.ids`）に入らなかった残りの全部を使う。AMI の turn の終わりは人の付けた境界なので、en の遅れは記録だけにする。threshold、B の `min_silence`、完結の表、RP-66 の上限、数え方の 8 から 10 は、手番 1c を流す前に固定し、結果を見て動かさない。手番 1 の held-out（`testdata/fleurs_bench/ja` の 81 本と AMI の 60 turn）は、結果を見た後なので、以後 held-out として使わない。手番 1c の結果は `docs/results/r8v/heldout2.md` に書く。**手番 2 の brief に写す条件**：A に流れの途中で `flush` も `reset` も打たない（3.17 の方針 4）。`Engine::finish` は A と B を `flush` して segment を取った後、2 本とも作り直して位置の offset を進める（`Vad::rebuild` と同じ形。10 の H75）。RP-68 の test を書いて通してから、`endpoint.rs`、`Vad` の 2 本目、`feed` の候補と完結の判定、確定の線、持っておいた文の使い回し、`asr.endpoint.decided`、`--endpoint` を作る。B だけの割れは、RP-66 で `finals` が 2 以上の標本として記録し、数え方の 8 で仕分けて文の途中の切断の率を合否に使う。`Final.speaker_audio` は A の声の始まりから確定の線までの sample から取る。ja の限度は 3.17 の「RP-66 と RP-67 の数え方」の 5 の式で置き直してから手番 3 へ進む。**手番 3**：RP-66 の run と、RP-1、RP-2、RP-5、RP-15 の打ち直し（区切りが変わるので） | RP-68（手番 2 の最初）、RP-66。RP-1、RP-2、RP-5、RP-12、RP-15、RP-25、RP-65 が今の main と同じく通る | 起こし直す時に `--endpoint silence`。code は R8v の commit を `git revert` |
| R8w | 訳の段の詰めの続き（3.18）。1：RP-32 の判定に訳の取れた割合を入れ、port の移った run を無効にし、RP-32 の run の手順に port の空きの待ちを足す（RP-72）。2：部分訳の使い回し（RP-69）と、その品質の run（RP-70。E2E の席）。3：推測デコードの llama-server の probe（RP-71。腕は N0 と S3。S3 は公開の GGUF を使い、語彙の一致と GPU の memory を確かめてから流す）と、S3 が通った時の unit の file と `hayamimi llama start` の引数。2 と 3 は触る物が重ならないので別の席で並べてよい。4：2 と 3 の build で RP-32 の Rust の run と RP-33、RP-25、RP-35 を打ち直す（E2E の席。起こす形は `sudo -n nice -n -5 <command>`） | RP-32、RP-69、RP-70、RP-71、RP-72。RP-33、RP-25、RP-35 は 4 の run で打ち直す | 1 と 2 は commit を `git revert`。3 は `systemctl --user stop hayamimi-llama-pair.service` を打ち、中継を `--llm` を付けずに起こし直す。`hayamimi llama start` の引数は commit を `git revert`。前の起動の引数は `docs/results/r8w/server-args.md` に在る |
| R3e | session の言語の文字を持たない 1〜2 字の確定文を捨て、`asr.final.dropped` を出す（10 の H67）。規則は `hayamimi` の確定文を出す所に置き、認識器と振り分けは変えない。VAD（Silero、`hayamimi-asr/src/vad.rs`、threshold 0.5、最短の発話 0.25 秒）は既に認識の前段に在り、`ل` の発話（1.824 秒）も VAD が声と読んで通した。VAD を前段に置くと認識の幻覚が大きく減る（S24）が、それは今の構成で既に効いており、threshold は変えない（変えると `L_asr` と RP-1 の誤り率が動き、測り直しが要る）。1〜2 字という字数の閾値に一次資料は無いので、字数だけでなく「session の言語の文字を 1 つも持たない」を併せて条件にし、本物の短い発話を落とさないことを RP-65 の陽性対照で確かめる。同じ席が、その発話が `tier` `omni` で解かれた理由を振り分けの log で読み、結果を H67 に返す。席は `hayamimi` の確定文の経路を触る実装の席 1 つ、波は 13:55 の組の後、見積りは 45 分 | RP-65 | commit を revert |
| R8o | H53 の決め（評議の判定）。2（観測）：`translate.line.held` に T の側の理由を enum で足し、止めた訳の本文は log に載せず、`Observer` の `draft_verify_input` と `verify_input` から repo の外の dev 用の file へ出す（`hayamimi-relay live --verify-text <file>` を付けた時だけ。実装は `native/hayamimi-relay/src/verify_dump.rs`、T の理由は `t_hold_reason`）。1（同じ run）：`verify_markers.toml` の `ja.negation` に「なさそう」「なさげ」「なさすぎ」を足す（「なさ」だけは「なさい」「なさる」「ごめんなさい」を巻き込むので足さない）。両方止めた行は今までどおり訳を出さない。新しい held-out の組は test-designer の席が作る（RP-44） | RP-42、RP-43、RP-44 | commit を revert |
| R3d | T-RP-1-013 の陰性対照の口：`hayamimi-eval scorecard` に、ja の clip を SenseVoice の tier へ送る評価用の引数を足す（配る `hayamimi` には足さない） | T-RP-1-013 | commit を revert |
| R8l | zh、ko、es の V の数の検査で、算用数字の後の倍の語の列を読む（3.6）。語だけで書いた数は 10 の H51 | RP-39 | その commit を `git revert` |
| R0b | worktree に `testdata/` が無いとき、test の support が本体の `testdata/` を引く。`hayamimi-eval` の `tests/support/mod.rs` の `BASE` と `TESTDATA` を、まず `CARGO_MANIFEST_DIR` の 2 つ上の `testdata`、無ければ `git rev-parse --path-format=absolute --git-common-dir` の親の `testdata` の順で探す関数にする。両方に無ければ、探した 2 つの path を名指しして `Err` を返す（黙って通さない）。symlink は使わない（Windows から WSL の symlink を辿れない。RP-24 の os error 1920） | worktree の中で `cargo test -p hayamimi-eval --test rb01 … --test rb08` の FAILED が 0。陰性対照は、本体にも worktree にも `testdata/` が無い clone で、2 つの path を名指しした `Err` で落ちる | commit を revert |
| R10 | `models fetch`、`config dump`、`doc-links`、README（`README.md` と `README.ja.md` の CPU だけという売りの行を含む）と文書の書き替え、英語版。`docs/guide/embedding.md` と `embedding.ja.md` の wire の記述を、その時の実装（3.3 の項目、`settled`、`of: "partial"`、`speaker` の event）に合わせる。T-RP-21-001、005、006、007 は `native/hayamimi/tests/rp21.rs` に置き、007 が使う RP-9 の `check_descendants` を R10 が作る | RP-21、RP-19 の spec の行、RP-30 | 文書の commit を revert |
| R12a | RP-19 と RP-9 の受入 test を作る（`native/hayamimi-eval/tests/rp19.rs` の `check_port_table` と、`rp09.rs` の Python を子に立てない確かめ。RP-9 の `check_descendants` は R10 の `native/hayamimi-eval/src/descendants.rs` を使う）。rp19 は R12a1、rp09 は R12a2 として 2 席に分ける。R12a1 は 13:55 の組の最後に main へ入れ、その時の main で未実装の一覧を作る。それより後の slice は自分の行を rp19 で確かめる | RP-19 の T の行（T-RP-19-001〜007）、RP-9 | commit を revert |
| R12b | RP-19 の 5.3 の未実装の行のうち、Python の file が `test_embed_runtime.py` の (a) の 29 本（行は 8 の「RP-19 の未実装 187 行の仕分けと slice」の行ごとの表で slice の列が R12b の行） | RP-19（その 29 行が list に在り、全部通る） | commit を revert |
| R12c | 同じく `test_ws_ingest.py`、`test_lifecycle.py`、`test_process_safety.py` の (a) の 25 本（取り込みの口と process） | RP-19（その 25 行） | commit を revert |
| R12d | 同じく `test_diar_eval.py`、`test_diarize.py` の (a) の 19 本（話者） | RP-19（その 19 行） | commit を revert |
| R12e | 同じく `test_itn_cjk.py` の (a) の 14 本（ITN） | RP-19（その 14 行） | commit を revert |
| R12f | 同じく `test_units.py` の (a) の 8 本（照合 V の数、preroll、置換辞書） | RP-19（その 8 行） | commit を revert |
| R12g | 同じく `test_audio_capture.py`、`test_ja_golden.py`、`test_asr_segment.py` の (a) の 16 本（取り込みの装置と ja の経路） | RP-19（その 16 行） | commit を revert |
| R12h〜R12v、R12a1、R12a2 | 同じ小節の (c) と材料の slice と、rp19 と rp09 | 同じ小節の表の受入 ID | commit を revert |
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
merge の前の検査では、受入の `-p <crate>` の run に加えて、branch を main に重ねた木の `native/` で `cargo test --release --workspace --no-run` を打ち、workspace の全 crate の test が build できることまでを確かめ、その最後の行を検証の返答に貼る。
`-p <crate>` だけの run は、別の crate の test が書いた struct の literal を compile しないので、struct に欄を足した変更が、その struct を literal で組む別の crate の test（`native/hayamimi/tests/r8a_relay.rs` の `RelayConfig`）を E0063 で壊したまま main へ入るからである。
この検査を打たせる機械の門は無い（CI の `.github/workflows/test.yml` は `pytest` を打ち、cargo を打たない。cargo に替えるのは R11 である）。

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
| R12a | 無し。T の 5.3 の対応表と `cargo test --release --workspace -- --list` | `rp19.rs` と `rp09.rs` が無い。5.3 の名前を持つ 334 行のうち list に在るのは 147 で、無いのは 187（test-designer の席の数え） |
| R12b〜R12g、R12h〜R12v | R12a1 の rp19（R12h、R12i、R12q、R12r、R12t、R12u は自分の受入 ID でも確かめる） | 各 slice が自分の行を rp19 で確かめる |
| R11 | RP-1（R3）、RP-13（R8f）、RP-15（R4）、RP-19 の全行（R12a1〜R12x。Refiner の R12w を含む）、R10 | 3.2 の R11 に入る 3 つの条件 |
| 端から端 | R3（`--lang`）、R5（取り込み）、R8a〜R8c、R6 の頁 | この小節の E2E-1 と E2E-2 |

R3 は 2 席に分ける。R3a は振り分け（`hayamimi-asr::route` と `::lid`、RP-19 の `route.rs` 70 行と `lid.rs` 9 行）、R3b は認識器と常駐（`::decode` と `::resident`、`decode.rs` 24 行、`hayamimi-eval scorecard`、RP-22）である。
R3a は R3b の `decode` を、言語の tier を受けて文字列を返す 1 つの関数として呼び、R3b が出来るまでは test の中の偽の関数で通す。

実装の席は `docs/design/status.md` と `status.ja.md` を書かない。slice を main へ merge した時に、設計の席がその slice の行の 4 列（状態、merge の commit、検証の出所、受入の通過）を英語版と日本語版の両方で書く（branch と main の間で status がぶつかるのを避けるため）。
status を書く同じ手番で、`README.md` と `README.ja.md` に在る数字（その日の時点の固定値）が status の値と食い違えば、README も直す。

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
| W4 | 04:45〜05:45 | R7 の置き場の測り直し（済み）。R11 は入口の条件（RP-19 の 187 本）が揃わないので、この波では入れない | 1 時間 |
| E2E | 05:45〜06:45 | 端から端の 2 本と `L` の p50、p95。`L` は RP-32 の run で取る | 1 時間 |
| W5 | 11:45〜13:55 | 8 の「RP-19 の未実装 187 行の仕分けと slice」の「13:55 までに入る組」（6 席、実装 11:45〜13:00、検証と merge 13:00〜13:55）。残りの slice は同じ小節の優先の順で次の波に並べる。R12 が全部入って RP-19 が通ってから R11 | 各 slice 60 分以内 |

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
| E2E-1 | llama-server、中継（`hayamimi-relay live --verify-text /home/fukugan/.cache/hayamimi-e2e-20261015/e2e1.verify.txt`。8 の R8s の 3）、process A を起こし、`<ev> stream --wav testdata/lat-ja.wav --repeat 5 --port 8766 --out e2e1.jsonl`、`cargo test --release --test e2e1 -- --ignored` | 有効な標本（3.1）のうち訳の取れた割合が 20 / 23 以上。`L`、`L_asr`、`L_tr` の p50 と p95 を出す。`L` は RP-3 の期待値（Python の p50 と p95 以下）で、RP-32 の固定した Python の基準の run と比べて判定する | 同じ test が `<base>` の Python の run から同じ列を出せる | llama-server を止めた run では訳が 0 件で test が落ち、`hayamimi` と中継は止まらず `translate.fallback.held` を出す |
| E2E-2 | llama-server、中継、process B を起こし、`<ev> stream --wav testdata/lat-en.wav --repeat 5 --sink loopback --out e2e2.jsonl`（既定の出力の装置へ鳴らし、送った sample の時刻を記録する口は R5 が足す）、`cargo test --release --test e2e2 -- --ignored` | `lang` が `en` の `final` と `lang` が `ja` の訳が出て、有効な標本のうち訳の取れた割合が 20 / 23 以上。`L` の p50 と p95 を出し、p95 が 2000 ms 以下（RP-3 の en→ja の値） | 同じ音声を `/ingest`（8767）へ送った run でも同じ列が出る | 出力の装置を止めた run では止まらず `capture.fallback.none` が 1 件 |

マイクの実機（RDP の録音の転送を通る音声）は、process A を起こして `capture.fallback.none` が 0 件で、マイクの装置の名前が log に出ることまでを機械で確かめる。
人が話した ja で en の字幕が出ることは、話す者が居る時にしか打てない（NOT VERIFIED）。
E2E の画面の確かめでは、R6 の頁に fallback の帯が出た状態（llama-server を止めた陰性対照の run）の DOM で、帯の高さと `role="status"` を R6 の板（`r6_board.ja.md` の 4.3）と照らす。R6 の画面テストはこの状態を測っていない。

| ID | 受入 test の名前 | 置き場 |
|---|---|---|
| E2E-1 | `e2e1_mic_process_ja_to_en_beats_python` | `native/hayamimi-eval/tests/e2e1.rs` |
| E2E-2 | `e2e2_loopback_process_en_to_ja_within_budget` | `native/hayamimi-eval/tests/e2e2.rs` |

2 本とも `#[ignore]` を持ち、process も llama-server も起こさず、run が残した記録を読んで判定する。
読む記録は git 管理下の決まった dir に置く。E2E-1 は `docs/results/e2e/e2e1/`、E2E-2 は `docs/results/e2e/e2e2/` である（RP-32 が `docs/results/r8h_verify/` を読むのと同じ形）。
dir の中は、Rust の samples が `e2e1.jsonl`（E2E-2 は `e2e2.jsonl`）で、E2E-2 の events がその隣の `e2e2.events.jsonl` である。
E2E-1 の Python の基準は同じ dir の `py1.jsonl`、`py2.jsonl` のように `py<N>.jsonl` と書き、1 本以上を置く。
生の run の残り（stdout、stderr、pid、gpu の csv）は git の外の `~/.cache/hayamimi-e2e-<日付>/` に置き、この dir には samples と events の jsonl だけを写す。
有効の判定（3.1、`--min-silence` 300 ms）、訳の取れた割合（20 / 23）、p50 と p95 の比べ方は、RP-32 と同じ `hayamimi_eval::compare` の関数を使う。
`L` と `L_tr` は有効な標本のうち訳の取れた物で、`L_asr` は有効な標本で取る。
打ち方は `cargo test --release --test e2e1 -- --ignored --nocapture` と、`--test e2e2` の同じ形である。
有効な標本の数、訳の割合、`L`、`L_asr`、`L_tr` の p50 と p95、可否を 1 行ずつ出す（`--nocapture` が無いと、通った時の数は出ない）。
E2E-2 は、events に `lang` が `en` の `final` と、`lang` が `ja` で字幕に出る訳が 1 件以上あることも判定する。
陽性対照は `docs/results/r8h_verify/` の記録（rs1、py1、py2、e2e2c）で、数が出ることまでを見る。
陰性対照は訳が 0 件の fixture（`native/hayamimi-eval/tests/fixtures/e2e_no_translation.jsonl`）で、落ちることを見る。
load の記録と、RP-32 の行ごとの有効の線は、この 2 本の判定に入れない（2 本は全標本に `min_silence` を渡す。10 の H88）。
RP-32 の判定の test（`native/hayamimi-eval/tests/rp32.rs`）は、git 管理下の `docs/results/rp32/` を読む。
dir の中は、固定した Python の基準の `py1.jsonl`、`py2.jsonl`、`py3.jsonl` と、Rust の `rs1.jsonl`、`rs2.jsonl`、`rs3.jsonl`（`stream --out` の samples）、Rust の run ごとの `rs<N>.events.jsonl` と `rs<N>.hayamimi.stdout.log`、対 N ごとの `load<N>.txt` である（3.16 の「RP-32 の dir の読み方」）。
`load<N>.txt` の 1 行は、run の名前、`before` か `after`、時刻、`/proc/loadavg` の 1 行を空白で並べた形（`docs/results/r8h_verify/loadavg.txt` と同じ）で、各 run の前と後に 1 行ずつ置く。
記録が揃っていない時、`--ignored` の test は足りない file の名前を挙げて落ちる。

### RP-19 の未実装 187 行の仕分けと slice

T の 5.3 の対応表で Rust の test の名前を持つ 334 行のうち 187 行は、その名前の関数が `native/` に無い。
この小節は 187 行を行ごとに 4 つへ分け、作る行を 60 分以内の slice に割り、移さない行の理由を置く。
(a) の行は Python の file で割った R12b〜R12g（8 の表）が入れ、この小節の slice は (c) の機能と材料と、rp19 と rp09 だけを持つ。

**数え方**（数えるのは実装の席と検証の席で、設計の席は cargo を打たない）：

```
LC_ALL=C
names = 5.3 の表の行のうち 3 列目が `-` でない物の 3 列目（awk -F'|' で切り、空白と backquote を除く）
have  = native/ の .rs の `fn <名前>` の名前（rg -o --no-filename --glob '!target' 'fn [a-z0-9_]+' native）
missing = comm -23 <(sort -u names) <(sort -u have)
```

- 範囲は worktree の `docs/design/rust_port_tests.ja.md` の 5.3 と `native/`（`target/` を除く）。
- 結果は 386 行、名前を持つ 334 行、そのうち `native/` に無い 187 行。
- 陽性対照は `join_pieces_concatenates_cjk_without_spaces`（在る名前）が `comm -12` で 1 行出ること、陰性対照は在りえない名前が 0 行であること。
- この式は `fn` の名前だけを見るので、test でない同じ名前の関数でも在ると数える。合否は `cargo test --release --workspace -- --list` の名前で決める（RP-19 の command）。

**4 つの分け方**：

- **(a) 移す**：Rust に同じ振る舞いの code が在り、test を書けば通る行。111 行。
- **(c) 作る**：Rust の `src/` に機能が無く、test だけでは届かない行。57 行（うち Refiner の 17 行は R12w で作る）。
- **(b) 写さない（Python の口）**：Rust が意図して持たない Python の口の行。15 行。RP-19 の 5 つ目の種類にする（6 の RP-19）。
- **材料が無い**：test の入力の音声が repo にも手元にも無い行。4 行。作る（R12u）。

111 + 57 + 15 + 4 = 187 である。
6 席の申告の足し算（a 113、b 14、c 54、材料無し 4、割れる 2）とは、行の置き方が 3 か所違う。この表が正で、違う行は次のとおりである。
`live_vad_failed_rebuild_preserves_active_and_pending_state` は (c)（作り直しの失敗を注入する口が無い）に置いた。
Refiner の 17 行は (c) の R12w に、recluster の 4 行は (c) の R12x に置いた。
`cli_rejects_loopback_on_non_windows_before_loading_models` ほか取り込みの 4 行は (b) に置いた。
割れた 2 行は、この表のどれかの行に入っている（名指しは 6 席の返しに無く、照らしていない。10 の H62）。

**(c) と材料が無い行は「写さない」に入れない**。
rp19 は、名前を持つ行のうち list に無い行を `native/hayamimi-eval/tests/rp19_not_yet.txt` の名前つき一覧で持つ（1 行に 1 名前、slice ごとに空行で区切った塊にし、席が違えば違う塊を消すので git の衝突を起こさない）。
一覧の行の数の上限を `rp19.rs` の `ALLOWED_NOT_YET` に置き、上限は下げる向きにだけ動かす（`rust_guide_contract.rs` の予算と同じ形）。
`rp19_python_cases_are_ported` は一覧が空でない間は落ち、一覧の数を出す（RP-19 は通っていないと機械が言う）。
一覧を作るのは R12a1 の席だけで、R12a1 が main に入った後の slice は自分の行を一覧から消し、`rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks` で、一覧に無い行が list に在り、一覧に在る名前が list に無いことを確かめる。

#### 行ごとの仕分け

(b) の 15 行は下の「写さない行」の表に在る。残る 172 行をここに置く。
分類の `a` は (a)、`c` は (c)、`材料` は材料が無い行である。

| case | 分類 | slice |
|---|---|---|
| `live_path_preroll_keeps_utterance_initial_words` | a | R12g |
| `split_retry_stands_down_when_the_speech_measure_clears_the_buffer_alarm` | a | R12g |
| `aligned_speaker_match_discards_stale_and_retains_future` | a | R12g |
| `capture_timestamps_keep_audio_spacing_when_backend_returns_a_burst` | a | R12g |
| `mic_timestamp_is_taken_in_the_capture_callback` | a | R12g |
| `microphone_device_selection_uses_input_only_case_insensitive_match` | a | R12g |
| `mix_speaker_failure_falls_back_once_to_mic_only` | a | R12g |
| `mix_waits_for_a_closer_packet_instead_of_consuming_older_candidate` | a | R12g |
| `put_latest_keeps_only_newest_bounded_items` | a | R12g |
| `speaker_capture_owns_com_objects_and_downmixes_native_channels` | a（downmix だけ） | R12g |
| `speaker_capture_rejects_non_windows_before_import_or_com` | a | R12g |
| `wait_for_speaker_drains_packet_enqueued_as_done_becomes_true` | a | R12g |
| `windows_com_scope_balances_every_success_result` | c | R12r |
| `windows_com_scope_uses_existing_changed_mode_apartment` | c | R12r |
| `windows_com_scope_rejects_other_hresult_failures` | c | R12r |
| `speaker_capture_failure_is_reported_with_endpoint_context` | c | R12r |
| `parse_rttm_basic` | a | R12d |
| `parse_rttm_ignores_non_speaker_lines_and_blanks` | a | R12d |
| `parse_rttm_empty_file` | a | R12d |
| `parse_rttm_rejects_malformed_line` | a | R12d |
| `parse_rttm_rejects_negative_duration` | a | R12d |
| `parse_rttm_round_trips_zero_duration` | a | R12d |
| `segments_to_der_tuples_coerces_types` | a | R12d |
| `segments_to_der_tuples_drops_zero_and_negative_length` | a | R12d |
| `segments_to_der_tuples_empty` | a | R12d |
| `der_zero_for_identical_ref_and_hyp` | a | R12d |
| `der_nonzero_for_swapped_speaker_labels_within_overlap` | a | R12d |
| `der_end_to_end_from_parsed_rttm` | a | R12d |
| `group_segments_single_segment` | a | R12d |
| `group_segments_merges_close_segments` | a | R12d |
| `group_segments_splits_on_silence_gap` | a | R12d |
| `group_segments_splits_on_max_length` | a | R12d |
| `group_segments_empty_input` | a | R12d |
| `der_breakdown_zero_for_identical_ref_and_hyp` | c | R12q |
| `der_breakdown_isolates_confusion_from_miss` | c | R12q |
| `production_over_splits_relative_to_eval_replica_on_a_skipped_vad_segment` | c | R12w |
| `pool_audio_for_group_uses_diarizer_turns_when_present` | c | R12x |
| `pool_audio_for_group_falls_back_to_full_buffer_when_declined` | c | R12x |
| `pool_audio_for_group_declined_duration_prefers_group_span_over_buffer_length` | c | R12x |
| `pool_audio_for_group_empty_group_yields_empty_audio` | c | R12x |
| `group_diarizer_missing_model_raises_filenotfound` | a | R12d |
| `group_diarizer_short_buffer_returns_empty` | a | R12d |
| `group_diarizer_finds_multiple_speakers_on_two_speaker_audio` | 材料 | R12u2 |
| `eventhub_publish_delivers_to_subscriber_in_order` | a | R12b |
| `eventhub_unsubscribe_stops_delivery` | a | R12b |
| `eventhub_multiple_listeners_all_receive_the_event` | a（複数の購読者） | R12b |
| `eventhub_replays_refine_events_to_new_subscribers` | c | R12w |
| `final_event_carries_audio_s_lid_ms_decode_ms` | a | R12b |
| `final_event_switched_is_false_for_the_first_final` | c | R12i |
| `final_event_switched_true_when_language_changes` | c | R12i |
| `final_event_switched_false_when_language_repeats` | c | R12i |
| `session_stats_reset_clears_last_final_lang` | c | R12i |
| `set_punctuate_off_then_on_is_a_noop_when_already_loaded` | a | R12b |
| `live_vad_applies_pending_change_at_a_safe_boundary` | a | R12b |
| `live_vad_defers_rebuild_while_speech_is_active` | a | R12b |
| `live_vad_set_sensitivity_partial_args_keep_other_values` | a | R12b |
| `live_vad_composes_pending_updates_and_keeps_last_explicit_value` | a | R12b |
| `live_vad_empty_update_does_not_rebuild_detector` | a | R12b |
| `live_vad_waits_for_completed_segment_to_be_drained` | a | R12b |
| `live_vad_rebuild_preserves_absolute_segment_positions` | a | R12b |
| `live_vad_failed_rebuild_preserves_active_and_pending_state` | c（注入の口） | R12v |
| `live_vad_rebuild_keeps_stream_preroll_and_refiner_coordinates` | c | R12w |
| `live_vad_real_speech_matches_fresh_detector_after_rebuild` | a | R12b |
| `live_vad_set_sensitivity_rejects_invalid_values` | a | R12b |
| `live_vad_set_sensitivity_accepts_valid_values` | a | R12b |
| `live_vad_set_sensitivity_none_is_always_a_noop_no_change` | a | R12b |
| `post_config_invalid_vad_value_returns_400_not_a_crash` | a | R12b |
| `audio_history_clear_empties_buffer_and_offsets` | c | R12h |
| `refiner_reset_drops_spans_and_clears_history` | c | R12w |
| `refiner_reset_writes_a_separator_into_an_open_transcript` | c | R12w |
| `reset_live_session_emits_summary_then_reset_in_order` | a | R12b |
| `reset_live_session_tolerates_every_optional_piece_being_none` | a | R12b |
| `reset_live_session_flushes_pending_refine_before_summarizing` | c | R12w |
| `control_queue_submit_does_not_run_synchronously` | a | R12b |
| `control_queue_poll_runs_queued_callables_in_fifo_order` | a | R12b |
| `control_queue_poll_with_nothing_queued_is_a_noop` | a | R12b |
| `control_queue_poll_sets_the_event_after_running` | a（`oneshot` の返事） | R12b |
| `run_stream_applies_a_submitted_task_on_the_decode_thread_not_the_submitter` | a | R12b |
| `get_config_reports_current_values` | a | R12b |
| `post_config_applies_a_subset_of_keys` | a | R12b |
| `post_config_invalid_value_returns_400_with_message` | a | R12b |
| `post_reset_calls_the_reset_function` | a | R12b |
| `post_reset_pending_returns_202` | a | R12b |
| `only_applies_to_ja_zh_yue` | a | R12e |
| `empty_text_is_a_noop` | a | R12e |
| `magnitude_form_full_arithmetic_without_big_units` | a | R12e |
| `magnitude_form_keeps_man_and_oku_as_literal_units` | a | R12e |
| `traditional_wan_and_liang_forms` | a | R12e |
| `decimal_digit_by_digit_reading` | a | R12e |
| `decimal_with_magnitude_integer_part` | a | R12e |
| `bare_digit_run_reads_digit_by_digit` | a | R12e |
| `single_bare_numeral_untouched` | a | R12e |
| `idioms_and_proper_nouns_stay_untouched` | a | R12e |
| `default_exclusion_list_blocks_a_longer_run_directly` | a | R12e |
| `user_exclude_adds_to_builtin_list` | a | R12e |
| `user_force_wins_over_rule_output` | a | R12e |
| `user_force_can_apply_to_text_the_rules_would_leave_alone` | a | R12e |
| `golden_file_is_self_consistent` | a | R12g |
| `golden_wavs_are_the_ones_that_were_recorded` | a | R12g |
| `references_still_match_the_fleurs_manifest` | a | R12g |
| `pipeline_still_produces_the_golden_text` | a | R12g |
| `spec_file_exists_and_declares_its_generator` | c | R12s2 |
| `config_matches_the_live_dump` | c | R12s1 |
| `dump_needs_no_models_directory` | c | R12s1 |
| `refiner_close_ends_the_worker_thread` | c | R12w |
| `refiner_close_is_idempotent` | c | R12w |
| `refiner_close_runs_the_queued_backlog_first` | c | R12w |
| `refiner_refuses_work_after_close` | c | R12w |
| `closing_the_refiner_releases_the_engine` | c | R12w |
| `refiner_close_closes_the_transcript_file` | c | R12w |
| `build_vad_raises_not_exits_when_model_missing` | a | R12c |
| `run_stream_stops_mid_wav_when_stop_event_is_set` | a | R12c |
| `run_stream_without_stop_event_runs_to_completion` | a | R12c |
| `mic_chunks_notices_stop_event_without_audio` | a | R12c |
| `reconstruct_inserts_period_and_comma_after_token_end` | c | R12t1 |
| `reconstruct_inserts_question_mark` | c | R12t1 |
| `reconstruct_inserts_exclamation_mark` | c | R12t1 |
| `reconstruct_no_mark_is_noop` | c | R12t1 |
| `reconstruct_ignores_special_token_offsets` | c | R12t1 |
| `reconstruct_only_first_mark_per_position_wins` | c | R12t1 |
| `reconstruct_multi_sentence_marks_mid_and_end` | c | R12t1 |
| `punct4_labels_order` | c | R12t1 |
| `decode_gates_remove_weak_marks_before_argmax` | c | R12t1 |
| `restore_smoke_question` | c | R12t2 |
| `restore_smoke_exclamation` | c | R12t2 |
| `restore_smoke_only_inserts_marks` | c | R12t2 |
| `restore_smoke_empty_string` | c | R12t2 |
| `missing_model_dir_raises_file_not_found` | c | R12t2 |
| `fast_path_default_finds_both_real_speakers` | 材料 | R12u2 |
| `provisional_display_does_not_reproduce_section_9s_swallowing_bug` | 材料 | R12u2 |
| `hysteresis_can_swallow_a_rare_real_speaker` | 材料 | R12u2 |
| `digits_pass_when_source_has_no_ascii_digits` | a | R12f |
| `digits_pass_on_exact_and_superset` | a | R12f |
| `digits_fail_on_mangled_number` | a | R12f |
| `preroll_prepends_context` | a | R12f |
| `preroll_never_bleeds_into_previous_segment` | a | R12f |
| `preroll_respects_rolling_buffer_offset` | a | R12f |
| `load_replacements_formats` | a | R12f |
| `load_replacements_empty_path` | a | R12f |
| `resolve_punct_model_accepts_known_values` | c | R12t3 |
| `resolve_punct_model_rejects_unknown_value` | c | R12t3 |
| `punct_property_defaults_to_bert` | c | R12t3 |
| `punct_property_builds_4class_when_selected` | c | R12t3 |
| `punct_property_4class_missing_falls_back_to_bert_with_warning` | c | R12t3 |
| `punct_property_disables_when_both_models_unavailable` | c | R12t3 |
| `punct4_prefers_int8_when_present` | c | R12t3 |
| `refiner_add_span_keeps_same_language_in_one_group` | c | R12w |
| `refiner_add_span_splits_group_at_language_boundary` | c | R12w |
| `refiner_add_span_splits_again_on_return_to_original_language` | c | R12w |
| `refiner_worker_prints_groups_in_enqueue_order_despite_slower_first_decode` | c | R12w |
| `refiner_shutdown_must_drain_full_backlog_not_just_last_task` | c | R12w |
| `compute_accept_key_matches_rfc_example` | a | R12c |
| `parse_handshake_request_extracts_path_and_key` | a | R12c |
| `parse_handshake_request_rejects_non_websocket` | a | R12c |
| `parse_handshake_request_rejects_missing_key` | a | R12c |
| `build_handshake_response_contains_correct_accept` | a | R12c |
| `build_handshake_request_roundtrips_through_parse` | a | R12c |
| `frame_roundtrip_unmasked_binary` | a | R12c |
| `frame_roundtrip_masked_text` | a | R12c |
| `frame_masked_bytes_differ_from_plaintext_on_wire` | a | R12c |
| `decode_frame_incomplete_returns_none` | a | R12c |
| `decode_frame_two_frames_in_one_buffer` | a | R12c |
| `decode_frame_large_payload_uses_64bit_length` | a | R12c |
| `frame_close_and_ping_opcodes_roundtrip` | a | R12c |
| `decode_pcm16_mono_roundtrip` | a | R12c |
| `decode_pcm16_drops_trailing_odd_byte` | a | R12c |
| `decode_pcm16_averages_stereo_to_mono` | a | R12c |
| `decode_pcm16_empty_input` | a | R12c |
| `resample_linear_identity_when_rates_match` | a | R12c |
| `resample_linear_upsamples_to_expected_length` | a | R12c |
| `resample_linear_downsamples_to_expected_length` | a | R12c |
| `resample_linear_preserves_a_ramp_shape` | a | R12c |

`/ingest` の手順と frame の 13 行の置き場は、T の 5.3 の `hayamimi/src/ingest.rs` ではなく `hayamimi-eval/src/ws.rs` である。`hayamimi` の `/ingest` は axum の WebSocket（`ingest.rs` の `WebSocketUpgrade`）で手順と frame を持たず、手で書いた codec は測りの道具の側に在るからである。置き場の列の書き換えは T の席が持つ。
(a) の行は module の在り処までを確かめ、関数の一致は確かめていない。席が同じ振る舞いの関数を見つけられなければ、その行を (c) として設計の席へ返す（10 の H63）。

#### 作る slice

見積りは 1 席、cargo の並列 2、release の build を 1 回含む分である。根拠は変える file と関数の数と、書く test の本数である。

| slice | 中身 | 変える file と関数 | 見積り | 優先 | 受入 ID | 受入 test の名前 |
|---|---|---|---|---|---|---|
| R12a1 | rp19 の照合。5 種の `写さない`、`rp19_not_yet.txt`、`ALLOWED_NOT_YET` | `native/hayamimi-eval/tests/rp19.rs`（新規）、関数 3（表を読む、list を読む、照らす）、一覧の file 1 | 55 分 | 0（他の slice の門） | RP-19 | `rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks` |
| R12h | session の reset が、前の session の音声の履歴、言語の粘り、話者を忘れる | `hayamimi-asr/src/audio.rs` の `History::clear`、`pipeline.rs` の `Engine::reset_session` と 3 つを受ける小さな関数 `reset_parts`、`hayamimi/src/session.rs` の `reset` が呼ぶ。3 file、関数 3 | 40 分 | 1 | RP-55 | `reset_parts_forget_the_previous_sessions_audio_language_and_speakers` |
| R12i | `final` の `switched` を前の `final` の言語から決める | `hayamimi/src/session.rs` の `Stats` に `last_final_lang` と `switched_to`、`wire.rs` の `final_event` に引数 1。2 file、関数 2 | 30 分 | 2 | RP-56 | `final_event_switched_true_when_language_changes` |
| R12v | VAD の作り直しの失敗を注入する口（`live_vad_failed_rebuild_preserves_active_and_pending_state`） | `hayamimi-asr/src/vad.rs` に test の build だけで効く注入の口 1 と test 1 | 25 分 | 3 | RP-19 | 同上（R12a1 の名前） |
| R12q | DER の Miss、FA、Confusion の内訳 | `hayamimi-eval/src/der.rs` に `breakdown` 1 と内訳の struct 1 | 35 分 | 5 | RP-57 | `der_breakdown_isolates_confusion_from_miss` |
| R12r | Windows の COM の初期化の HRESULT を分ける。`CaptureError` に endpoint の名前 | `capture/windows.rs` に HRESULT を分ける純関数 1 と scope の型 1、`capture/mod.rs` の `CaptureError` の欄 1。2 file | 55 分 | 6 | RP-58 | `windows_com_scope_uses_existing_changed_mode_apartment` |
| R12s1 | `hayamimi config dump` | `args.rs` の subcommand、新しい `config_dump.rs` の関数 1、起動の分岐 1。3 file | 50 分 | 6 | RP-19 の spec の行 | `config_matches_the_live_dump` |
| R12s2 | `docs/spec/ja_pipeline_spec.json` を `config dump` で作り直し、`generated_by` を `hayamimi config dump` にする | data の file 1、test 1 | 25 分 | 6（R12s1 の後） | RP-19 の spec の行 | `spec_file_exists_and_declares_its_generator` |
| R12t1 | 4 クラスの句読点の純関数（`reconstruct_*` 7、`punct4_labels_order`、`decode_gates_remove_weak_marks_before_argmax`） | 新しい `hayamimi-asr/src/punct/four.rs`、関数 3 | 50 分 | 7 | RP-59 | `reconstruct_inserts_question_mark` |
| R12t2 | 4 クラスの句読点の session（`restore_smoke_*` 4、`missing_model_dir_raises_file_not_found`） | `punct/four.rs` に開く関数 1 と復元 1。model は `models fetch --punct-4class` が取る | 60 分 | 7（R12t1 の後） | RP-59 | `restore_smoke_question` |
| R12t3 | `--punct-model {bert,4class}` の選択と落とし（`resolve_punct_model_*` 2、`punct_property_*` 4、`punct4_prefers_int8_when_present`） | `args.rs`、`punct/mod.rs`、関数 2 | 50 分 | 7（R12t2 の後） | RP-59 | `punct_property_4class_missing_falls_back_to_bert_with_warning` |
| R12u1 | 2 人の話者の音声の材料を AMI の RTTM から作る | `hayamimi-eval` に切り出しの関数 2（窓を選ぶ、wav と manifest を書く） | 45 分 | 8 | RP-60 | `two_speaker_clip_is_cut_by_the_rule_from_ami` |
| R12u2 | 材料を読む 4 行 | `diarize.rs`、`speaker.rs` に test（model が要るので `--ignored`） | 40 分 | 8（R12u1 の後） | RP-60 | `group_diarizer_finds_multiple_speakers_on_two_speaker_audio` |
| R12a2 | RP-9 の確かめ | `native/hayamimi-eval/tests/rp09.rs`、R10 の `descendants.rs` を使う | 40 分 | 8 | RP-9 | `rp09_no_python_at_run_time` |
| R12y | RP-35 の held-out の未実装の引数：`hayamimi-relay eval` の `--final`、`--heldout <path>`、`--direction <ja-en\|en-ja>` と、会話字幕の CS-30 の拒み（同じ `heldout_sha256` が別の `tables_sha256` で結果の file に在れば打たずに止まり、`eval.heldout.spent` を出す）。今の `parse_eval` は `--pairs`、`--out`、`--llm`、`--llm-other` だけを受け、原文を ja としてしか読まない（`eval.rs` の `verify_into` に `Lang::Ja`）ので、en→ja の held-out の入口が無い | `native/hayamimi-relay/src/cli.rs` の `parse_eval`、`eval.rs` に en を原文とする入口と held-out の digest の照らし、関数 3〜4 | 60 分 | 8 | RP-35 | `rp35_latency_work_keeps_chrf_and_verify` |

優先は速さ、品質の順で、同じ順の中は live の経路に近い物を先にした。
R12h が先頭なのは、今の `session.rs` の `reset` が `Engine::finish` と `Stats` の置き直しだけを行い、`History` の buffer、`Router::reset_session`（言語の粘り）、話者の重心をどれも消さないからである（code を読んで確かめた。`session.rs` の `reset`、`pipeline.rs` の `Route`、`audio.rs` の `History`）。
その結果、reset の後の最初の確定文は前の session の言語に粘って振り分けられ（違う言語なら言語判定の切り替えを 2 回待つ）、前の session の話者の印を受け、preroll は reset の前の音声を最大 1 秒取り込む。
振り分けの誤りは誤り率と `L_asr` の両方に効く。
`History::clear` は buffer を空にし、`last_segment_end` を今の終わりに置き、絶対の位置（`offset`）は戻さない。`t1_s` は process の流れの先頭からの位置で（3.1）、戻すと reset の前後で `t1_s` が重なるからである。
R12i は字幕の言語の印と mobile の consumer が読む値で、`wire.rs` が `switched` を常に偽で出している。
R12q は評価の道具だけ、R12r は Windows の取り込みの thread が自前で始めるので既に別の mode の thread に当たらない口、R12s は文書の出所、R12t は既定で切った opt-in なので、効く物の後に置いた。

#### 清書（Refiner）を live に足す形

今の Rust の live の経路は清書をしない。
`hayamimi-asr/src/pipeline.rs` に清書の呼び出しは無く、`refine_group` の呼び手は評価の道具の `hayamimi-eval/src/der_run.rs` の `refine_all` だけで、`session_summary` の `refine_*` は 0 の定数である。
Python は清書を既定で行い（`--no-refine` で切る）、3.2 は `realtime_transcribe.py` の Refiner を `hayamimi` へ移すと決めている。
したがって Refiner の 17 行は「写さない」にせず、作る。今日（13:55 の組）は作らず、下の形と slice を置く（評議の判定）。
17 行は作り終えるまで未実装の一覧に残り、R11 の門は動かさない。

**認識器の持ち方**：清書は live と別の `Router` を持つ。
`Router::refine(&mut self, …)`（`hayamimi-asr/src/route/refine.rs`）は `&mut self` を取るので、live と同じ `Router` で清書すると、25 秒（`refine.rs` の `GROUP_MAX_SECONDS`）の group を解き直す間、live の確定文がその `Router` を待つ。
確定の待ちは `L_asr` にそのまま乗り、速さが第一の目標に反する。
別の `Router` は認識器の model を 2 つ目として memory に読む。認識器は CPU（int8）に置くので（3.5）、増えるのは host の memory だけで、GPU の memory（RP-22）には触れない。
清書の `Router` は、清書する group の確定文の言語の tier だけを読み、常駐は 1 つ（`--max-resident` とは別に 1）にする。言語判定は読まず、group の言語を渡す。
採らない形は 2 つである。同じ `Router` を lock で分ける形は、上のとおり確定を待たせる。live の確定が空いた時だけ同じ `Router` で解く形は、2 音源が話し続けると清書が始まらず、group が溜まる。

**優先と取り分**：清書は 1 本の thread で、group を 1 つずつ解く。
認識器の thread の数は live の残りを取らないよう 2 にする（live の認識器の設定は変えない）。
live の窓が遅れている間（`session.rs` の `behind()` が真）は、次の group を始めない。始めた group は止めない。
待ちの列は 4 group までとし、溢れたら最も古い group の清書を捨て、その group は速報の確定文のまま残し、`refine.group.skipped`（warn）を 1 回出す。
捨てた group の数は `session_summary` に数える。

**group を分ける規則**：確定文を着いた順に group に足し、次のどれかで group を閉じる。
確定文の言語が group の言語と違う（違う言語の確定文から新しい group を始める）。
前の確定文の終わりから 2.0 秒（`GROUP_GAP_SECONDS`）以上の無音が在る。
group の長さが 25 秒（`GROUP_MAX_SECONDS`）を超える。
session の reset と終わりでは、開いている group を閉じて清書してから `session_summary` を出す。

**受入（RP-61）**：清書を入れた `hayamimi` で、RP-3 の run（rp03）と E2E-1、E2E-2 を打ち直し、`L` の p50 と p95 を 2 つと比べる。
1 つ目は、同じ負荷で交互に測った Python（清書を既定のまま入れた run）で、Rust の `L` の p50 と p95 がその値以下であること。
2 つ目は、清書を切った Rust の run で、清書を入れた Rust の `L` の p50 の悪化が 30 ms 以下、p95 の悪化が 60 ms 以下であること。
30 ms と 60 ms は実測の裏づけの無い仮の値で、R12w0 の測りの後に持ち主の R12w4 の席が置き直し、超えたら評議へ戻す（10 の H64）。
訳の取れた割合（RP-32 の 20 / 23）は清書の有無で下がらない。

**作る前の安い測り（R12w0）**：`<ev> der --set testdata/eval_diar --collar 0.25` を 1 回打ち、会議ごとの `refine_seconds` と `audio_seconds`（`der_run.rs`）から、清書 1 group の重さ（group の数で割った秒）と実時間比を出して `docs/results/` に書く。
これは話者の再分離の時間で、文の解き直しの時間を含まない。文の解き直しの重さは R12w2 の席が `Router::refine` を 25 秒の group に 5 回掛けて p50 を測る。
実時間比が 0.5 を超えれば（清書が音声の長さの半分より長く掛かれば）、待ちの列が溢れる見込みなので、R12w1 の前に設計の席へ戻す。

| slice | 中身 | 見積り | 席と波 |
|---|---|---|---|
| R12w0 | 上の安い測り | 15 分 | W6（14:00〜15:00）の席 1 |
| R12w1 | group を分ける規則（`refiner_add_span_*` 3）と、評価の複製との一致（`production_over_splits_relative_to_eval_replica_on_a_skipped_vad_segment`） | 45 分 | W6 の席 1（R12w0 の後） |
| R12w2 | 清書の thread、待ちの列、閉じる時の掃き出し（`refiner_close_*` 4、`refiner_refuses_work_after_close`、`closing_the_refiner_releases_the_engine`、`refiner_worker_prints_groups_in_enqueue_order_despite_slower_first_decode`、`refiner_shutdown_must_drain_full_backlog_not_just_last_task`） | 60 分 | W7（15:00〜16:00）の席 1 |
| R12w3 | `refine` の event、transcript の file、reset（`eventhub_replays_refine_events_to_new_subscribers`、`refiner_reset_*` 2、`reset_live_session_flushes_pending_refine_before_summarizing`、`live_vad_rebuild_keeps_stream_preroll_and_refiner_coordinates`） | 60 分 | W7 の席 2（R12w1 の後） |
| R12w4 | RP-61 の run（rp03、E2E-1、E2E-2 の打ち直し。Windows の実機） | 60 分 | W8（16:00〜17:00）の席 1 |

#### recluster の純関数（R12x）

`pool_audio_for_group_*` の 4 行は、音声と turn の引数だけで返す値が決まる純関数で、清書の時期に懸けない。
R12x は `hayamimi-asr/src/recluster.rs`（新規）に `pool_audio_for_group` を 1 つ置き、4 行を通す（30 分、受入 ID は RP-62）。
呼び手（`--global-recluster` の option）は R12w の後に作り、この slice には入れない。

#### 写さない行（Python の口）

RP-19 の 5 つ目の種類は `写さない：Python の口（<Rust の在り処、または持たない機構>）` とする。
ラベルだけで在り処も機構も無い行は、(a) か (c) に戻す。

| case | Rust の在り処、または持たない機構 |
|---|---|
| `cli_rejects_loopback_on_non_windows_before_loading_models` | 3.15 は要る物が無いことで起動を止めないので、loopback の無い OS では取り込みの落とし（`capture.fallback.single` か `none`、RP-29 の R5 の行）で動き続け、拒む口を持たない |
| `loopback_rejects_corrupt_channel_or_window_shapes` | Rust の loopback は `wasapi` が返す型つきの format（channel の数と frame の数）から窓を組むので（`capture/windows.rs`、`capture/convert.rs`）、形の壊れた配列を受ける口が無い |
| `mix_microphone_failure_is_fatal_and_stops_speaker` | 3.15 はマイクが開けない時に在る音源だけで動かし、`capture/combine.rs` は speaker だけの窓をそのまま流すので、致命にする形を持たない |
| `mix_stall_drops_both_sides_symmetrically_and_keeps_time_alignment` | Rust の mix はマイクの窓を時計にして同じ時刻の speaker の窓を足し（`capture/combine.rs`）、speaker の側が詰まってもマイクの窓を捨てないので、両側を捨てる形を持たない |
| `eventhub_add_listener_receives_the_raw_dict_synchronously` | Rust の `Hub`（`server.rs`）は broadcast の channel で文字列の event を配り、同じ process の中で同期に呼ぶ listener の口を持たない。埋め込む側は `/events` の SSE を読む（3.8） |
| `eventhub_listener_exception_is_swallowed_and_reported_once` | 同期の listener の口が無いので（上の行）、listener が投げる例外も無い |
| `eventhub_a_second_broken_listener_gets_its_own_report` | 同上 |
| `control_queue_poll_sets_event_and_continues_past_a_raising_callable` | Rust の命令は closure ではなく data の `Command`（`control.rs`）で、投げる命令が無い。適用の失敗は `session.rs` の `apply` の `Result` で loop に返る |
| `control_queue_task_runs_on_the_thread_that_calls_poll_not_the_submitter` | 命令が closure でないので、走る thread を問う物が無い。命令を decode の loop で適用することは `run_stream_applies_a_submitted_task_on_the_decode_thread_not_the_submitter`（R12b）が持つ |
| `run_stream_polls_control_once_per_chunk` | Rust の session の loop は窓ごとに届いた命令を全部適用する（`session.rs` の `apply_commands`）。1 窓に 1 回 poll する口は無い |
| `run_stream_without_control_is_unaffected` | Rust の `Session` は命令の受け口を常に持ち、受け口の無い形が無い |
| `config_and_reset_404_without_controls_attached` | Rust の server は `/config` と `/reset` を session の命令の受け口へ常に結び（`routes.rs`）、受け口を付けない server の形が無い |
| `post_reset_without_reset_fn_returns_400` | 同じ理由で reset の関数の無い形が無い。loop が止まった時は `routes.rs` の `reset` が `applied(false)` を返す |
| `set_punctuate_reenable_after_degrade_clears_punct_for_retry` | Rust は句読点の model を起動時に 1 回開き、開けなければ 3.15 の形で落として実行中に読み直さない。`Engine::set_punctuate` は on と off の切り替えだけで、再試行する状態を持たない |
| `run_clip_leaves_no_engine_behind` | Rust の `Engine` は持ち主の scope を出た時に `Drop` で手放され、process 全体から引く global や回収待ちの参照を持たない |

15 行である。T の 5.3 への移しと 5.2 の凡例の 5 つ目の種類は T の席が書く。

#### 材料が無い 4 行

`group_diarizer_finds_multiple_speakers_on_two_speaker_audio`、`fast_path_default_finds_both_real_speakers`、`provisional_display_does_not_reproduce_section_9s_swallowing_bug`、`hysteresis_can_swallow_a_rare_real_speaker` は `testdata/two_speakers.wav` を読み、その file は repo にも本体の `testdata/` にも無い（Python の側も skip している）。
材料は作る（R12u1）。
規則は、`testdata/eval_diar/` の AMI の 5 会議の RTTM を会議の名前の順に読み、話者がちょうど 2 人で重なりが無く、話者の区間が 4 つで、少ない方の話者の区間が 1 つ（2〜4 秒）である最初の窓を切り出すことである（Python の test の docstring が述べる「4 区間のうち 1 つが約 3 秒の 2 人目」の形）。
切った会議の名前、窓の始まりと終わり、音声の sha256 を manifest に書き、sha256 は記録だけにする。
作らずに外す形は採らない。話者の印は transcript の品質に効き、AMI の音声が手元に在るので材料を作る手数は 1 slice に収まるからである。
規則に合う窓が 5 会議に無ければ、R12u1 の席が窓の条件（区間の数、秒）と見つかった窓の数を設計の席へ返す。

#### 13:55 までに入る組

実装の窓は 11:45〜13:00、席は 6 で、席ごとに cargo の並列は 2 である。
6 席は R12b〜R12g の (a) の test を書き終えた席で、窓の初めにその build と直しを打ち、終えた席が続けて (c) の slice を 1 つ持つ。
(a) の build と直しの見積りは、書いた test の本数に 1 本 1 分と release の build 1 回の 10 分を足した値である（R12b 29 本で 40 分、R12f 8 本で 20 分）。
13:00〜13:55 は、書いていない席が受入を打ち直してから merge する。

| 席 | (a) の build と直し | 続けて持つ slice | 計 |
|---|---|---|---|
| R12b の席 | 40 分 | R12h（25 分に縮む見込み。`session.rs` と `pipeline.rs` を同じ席が続けて触るので、読み直しが要らない） | 65 分 |
| R12c の席 | 35 分 | R12x（30 分） | 65 分 |
| R12d の席 | 30 分 | R12q（35 分、同じ `der.rs`） | 65 分 |
| R12e の席 | 25 分 | R12v（25 分） | 50 分 |
| R12f の席 | 20 分 | R12a1（55 分） | 75 分 |
| R12g の席 | 30 分 | R12i（30 分） | 60 分 |

R12a1 は最後に merge する。`rp19_not_yet.txt` は、R12a1 の席が merge の直前の main で 5.3 と list を照らして作り、各 slice の席は一覧を書かない（一覧を 1 席だけが書くので、一覧の衝突が起きない）。
R12a1 が main に入る前の slice の検証は、`cargo test --release --workspace -- --list` にその slice の行の名前が在り、全部通ることで受ける。
merge の順は R12b〜R12g、R12h、R12i、R12q、R12v、R12x、R12a1 である。R12b、R12h、R12i は `session.rs` を触るので、この順で後の席が前の席の差に重ねる。
入らない slice は R12r、R12s1、R12s2、R12t1、R12t2、R12t3、R12u1、R12u2、R12a2 と、W6〜W8 に置いた R12w0〜R12w4 である。
入る組が全部入ると、未実装の行は 172 行（187 から写さない 15 を除いた数）から 172 − 111 − 1 − 4 − 2 − 1 − 4 = 49 行になる（COM 4、config dump 3、4 クラスの句読点 21、材料 4、Refiner 17）。R12a1 の `ALLOWED_NOT_YET` の最初の値は、その時に照らした行の数にする。
R11 はこの組では入らない。

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
- **H44** RP-5 の実走（T-RP-5-009）が落ちる。R8e の Rust の run の `t_partial` の p95 が 4398 ms で、同じ `--min-silence` 0.30 の Python の 3751 ms を超える。原因は特定していない（NOT VERIFIED）。持ち主は R8h で、3.16 の 3 の待ちの log と RP-32 の Rust の run で速報の遅れを Python の基準と並べ、差が残れば原因を評議へ出す。
- **H48** `eval.rs` の `settle_pair`（ja と en の間）は V を 2 回（T の訳と E の訳）掛ける。今は en の推量の表が無く（`verify.rs` の `table_for` が en で `None`）、推量の log は出ないが、en に推量の表を足すと、同じ行で `translate.hedge.logged` を 2 件出す。持ち主は en に推量の表を足す席で、足す同じ手番で `settle_pair` を確定の訳の V だけで log を出す形にし、RP-38 に en の行を足す。
- **H53** E2E-1 の Rust の訳の取れた割合が 67/80（0.8375）で、RP-32 と E2E-1 の 20/23（0.8696）に届かない（E2E の席の申告、`docs/results/r8h_verify/`。`--repeat 2` の 2 対で、設計の 5 回と 3 対より少ない）。`L` は Python の p50 1481 ms と p95 2018 ms に対し Rust が 1420 ms と 1950 ms で、`L` だけなら下回る。
原因（原因の席の申告を評議が記録から打ち直して一致）：抜けた 13 標本は 13 件とも T と E の両方が V に止められ、中継が何も出さずに閉じた行である（`pipeline.rs` の両方止めた行の扱い）。12 件は utt 11（`number`）、utt 16（`negation`。原文の「なさそう」が `verify_markers.toml` の `ja.negation` に無い）、utt 23（`uncertainty`。think と about a の仮説）の 3 発話が 2 run で 2 回ずつで、1 件は rs2b の utt 22（`number`）である。時間切れ、T の失敗、wire の欠けは 0 件である。
決め（評議の判定）：両方止めた行は今までどおり訳を出さない。止めた訳を出す形（a）は採らない。Zoom と Teams に出した字幕は取り消せず、20/23 の定義を変えるだけになり、wire の契約（`docs/guide/embedding.md` と `deliver.rs` の held の扱い）が割れるからである。止めた行に印は付けない（SRT は `〔訳を控えました〕` と原文、board は `ui.tr.held`、OBS は何も出さず、Zoom と Teams には送らない）。
順は R8o の 2（観測）、R8o の 1（否定の表、同じ run）、E2E の打ち直し、3（推量の扱い）である。3 は R8s の 1 で入れる（評議の条件つき GO）。ja→en の推量の不一致は止めずに log にだけ残し、en→ja は原文の推量が訳で落ちた時だけ止め、訳だけが推量を持つ時は log に残す（8 の R8s、R8t、RP-48、RP-63）。
- **H54** E2E-2（`/ingest` に送った en→ja。実の loopback ではない）の `L` の p95 が 3139 ms で、上限の 2000 ms を超える（n 25、訳 25/30、E2E の席の申告）。原因（原因の席の申告、記録の時刻で観測）：p95 は n 25 の nearest rank で上から 2 番目で、上位は r1u1 9341 ms、r1u2 3138 ms、r2u14 2279 ms である。
r1u1 は、英語の認識器 v3 を最初の英語の発話で初めて読み込む時間（`asr.model.loaded` の `load_ms` 7979.622）で、en→ja の session を始めるたびに 1 回起きる。続く r1u2 は、認識の窓の待ち（`asr.feed.waited` 3053、2016、325 ms、backlog の最大 95）と、訳の lane が 1 本で直列のための待ちである。
u14 は 2 回とも T の訳が V に落ち、E を待った（T 255 ms と E 1107 ms）。E2E-1 にも同じ形が rs1 と rs2b に 10 件ずつ在る。u14 の T が V のどの規則で落ちたかは確かめていない（R8o の 2 の log で読む）。
同じ記録で、en→ja の E の訳 35 件のうち 20 件が E の prompt の雛形（`English:`、`Draft Japanese:`、`Rewritten Japanese:`）を書き返した文のまま確定の訳として字幕に出ていた。ja へ向けた V の形の検査が、日本語の文字が 1 つでも在れば通すからである。ja→en は 46 件中 2〜4 件である。E が 1 秒ほど掛かるのも長い雛形を書くためと見ている（NOT VERIFIED）。
直しは R8p（雛形、最優先）、R8q（v3 の先読み）、R8r（新しい行の T を前の行の E の後ろに並べない）で、見込みは R8q だけで p95 約 2171 ms、R8p と R8q で約 1.4〜1.5 秒である（NOT VERIFIED）。確かめる run は `--repeat 5` で en→ja の訳を 60 件以上取り（n 60 なら p95 は上から 3 番目）、最初の発話の遅れは別の列に出す。実の loopback の run は R8m の後に打つ。
- **H56** `--verify-text` を渡さない時に dev 用の file が作られないことを確かめる test が無い（R8o の検証の席の所見）。持ち主は、RP-42 にその行を足す test-designer の席と、その行を通す実装の席である。入ったら、この穴を消す。否定でない「申し訳なさそう」「仕方なさそう」「情けなさそう」「頼りなさそう」は、否定の表の `exclude` に語幹（`申し訳な` ほか）で在る（`native/hayamimi-relay/data/verify_markers.toml`）。
- **H59** 否定の表の `exclude` は否定の側にだけ効き、推量の表の `そう` は「〜なさそう」の `さそう` に当たる。そのため `申し訳なさそうではない`、`仕方なさそうにしない` の否定を残した正しい訳も、訳が `seem`、`appear`、`look like` のどれも持たなければ（`does not look apologetic` の形）`Held(Uncertainty)` で止まる（R8o2 の検証の席の所見。機構は 2 つの表と `verify.rs` の `find_marker` を読んで確かめ、所見の訳の文字列は打ち直していない）。止まった T の訳は E を待ち、両方止まれば訳を出さない（H53）。
決め：表は変えない。ja→en のこの止まりは R8s の 1（推量の不一致を log にだけ残す、RP-48）で止まらなくなり、R8s の 1 が入ったらこの穴を消す。en の推量の表に `look` を足す形は採らない（`look at` のような推量でない語を持つ訳が、推量の無い原文で止まる）。推量の側の `exclude` に `申し訳なさそう` ほかを足す形も採らない（様子の `そう` を落とした訳を止めるのは表の決めのままだからである）。
機械が拾う物は 2 つである。E2E-1 の打ち直しの `translate.line.held` の T の側の理由（RP-42）が `uncertainty` の行を数える。unit の行は無く、RP-43 に、この 2 文の `seem` を持つ訳が通り `look` だけの訳が `Held(Uncertainty)` になることを固定する穴の行を足す（test-designer の席）。持ち主は R8s の 1 の実装の席で、RP-48 が通ったら RP-43 の 011 の固定の行が落ちるので、同じ手番で test-designer の席が書き直す。
- **H60** 札の読みを行頭に限った（8 の R8p）ので、次の形が残る。どれも記録には出ていない。`docs/results/r8h_verify/` の e2e2c、rs1、rs2、rs2b の `type` が `translation`、`of` が `final`、`stage` が `e` の訳 172 件を `jq` で抜いて数えると、太字の札 0、言語名と `:` の間に注を挟む札 0、番号つきの札 0、行の途中の札 0 である。同じ式は e2e2c の雛形を書き返した 20 件（うち行頭の `English:` を持つのは 8 件）と、植えた 4 つの形の陽性対照を数える。札の頭の綴りは `Draf`、`Drafted`、`Draft` と `Japanese`、`Japaese`、`English` で、言語名からの綴りゆれは 1 文字までである。
剥がしきれず字幕に出うる形は、太字の札（`**Rewritten Japanese:** 訳`）、言語名と `:` の間に注を挟む札（`Rewritten Japanese (final): 訳`）、番号つきの札（`1. Rewritten Japanese: 訳`）、1 行に全部が続く雛形（`English: 原文 Draft Japanese: 下書き Rewritten Japanese: 訳`）の 4 つである。ja へ向けた V の形の検査は日本語の字が 1 つでも在れば通すので、en→ja ではこの 4 つが確定の訳として字幕に出る。機械が拾うのは RP-45 の E2E-2 の打ち直しで札の語を位置を問わず数える行だけで、その run に出た時に限る。live の run で拾う物は無い。unit の行は、T の 4.55 の 012〜015 を、札の語が残ることを固定する穴の行に替えて置く（test-designer の席）。剥がす形を広げた手番でその 4 行が落ち、この項を書き直す合図になる。
剥がしすぎる形は、言語名から 2 文字以内の実在の別の語を言語名と読む形である。行頭の `Draft Javanese:`（en→ja、`Japanese` から 1 文字）か `Draft Enlist:`（ja→en、`English` から 2 文字）で始まる訳の行は札と読まれ、その行より前の訳が落ちる。後ろが空なら `form` で止まり、空でなければ削れた訳が V に渡る。機械が拾うのは `Javanese` の形を固定する T-RP-45-016 だけで、`Enlist` の形の行と live の run の検出は無い（`Enlist` の固定の行を足すのは test-designer の席）。
決め：どちらも code を変えずに残す。剥がす形を広げると行の途中の語を札と読む形へ戻り、札に似た正しい訳（`The draft: nobody read it.`）の頭を削るからである。許しを 1 文字に縮めると `Enlist` の形は閉じるが、`Javanese` の形は 1 文字違いで残り、合成の 2 文字の綴りゆれ（T-RP-45-011）を拾えなくなる。実在の言語名を除く表は部品を 1 つ足す。持ち主は E2E-2 の打ち直しの席で、札の語が 1 件でも数えられたら、その形を名指しして設計の席へ返す。
- **H61** 英語側の否定の表（`verify_markers.toml` の `en.negation`）は語と `n't` だけを読み、接頭辞の否定（`un-`、`dis-`、`in-`。表に在る `unable` を除く）を読まない。そのため原文の否定を接頭辞で訳した正しい訳（`興味がなさげな顔だった。`→`He looked uninterested.`）が `Held(Negation)` で止まる（T-RP-43-012 が固定する）。止まった T の訳は E を待ち、両方止まれば訳を出さない（H53）。
決め：止まりを受け、code も表も今は変えない。止まる向きは誤った訳を字幕に出さない側で、速さの損は E の 1 回の待ちである。接頭辞の規則（`un` で始まる語を否定と読む）は採らない。`under`、`until`、`unit`、`understand`、`discuss`、`information` のような否定でない語を否定と読み、否定の無い原文の訳を止める（否定の検査は有無の一致を見る。T-RP-43-008）からである。直すときは、記録に出た語だけを `en.negation` の `markers` に 1 語ずつ足す（`unable` と同じ形。表の data の変更で、code は要らない）。
機械が拾う物は、E2E-1 の打ち直しの `translate.line.held` の T の側の理由が `negation` の行と、`--verify-text` の file に出た訳の文字列である。持ち主は E2E-1 の打ち直しの席で、`negation` で止まった訳のうち接頭辞の否定の語を持つ行を数え、1 行以上なら語を名指しして設計の席へ返す。表に足す時は RP-43 の 012 の 2 件目が落ちるので、同じ手番で test-designer の席が書き直す。
- **H66** T と E が同じ数を作る誤訳をし、V が正しく `number` で止めて訳が欠ける行がある。観測の run（`/home/fukugan/.cache/hayamimi-e2e-20261015/` の `obs1-verify.txt` と `obs2-verify.txt`、`--repeat 2`）で、en→ja の `…had not as yet materialized…` が 2 回とも T と E の両方で「100%具体化したわけではなく」になり、ja→en の「波風よ立ってくれ」が T と E の両方で `"Waves of 1000 Feet"` になった。V の誤りではないので、V は変えない。
  訳し直し（8 の R8u）は作らない。probe（`/home/fukugan/.cache/hayamimi-e2e-20261015/r8u-probe-20261015.txt`、CAT-Translate-3.3b-Q8_0、`temperature` 0、腕ごとに 2 回）で、2 run の中で T が数を作った 24 行と、原文に数の在る対照の 14 行を打った結果が次の表である。腕 0 は今の T、腕 A は T に原文の数を名指す 1 文を足した依頼、腕 E0 は原文だけに E の指示を掛けた依頼、腕 E は E0 に同じ 1 文を足した依頼である（腕の中身は転記で、この席は依頼の本文を見ていない）。

  | 腕 | 数を作った行（24 行中、1 回目/2 回目） | 対照 14 行で数を作った行 | p50（ms、76 本） | 2 回とも同じ出力（38 行中） |
  |---|---|---|---|---|
  | 0 | 22/22 | 0 | 289 | 34 |
  | A | 17/18 | 0 | 257 | 32 |
  | E0 | 6/5 | 0 | 640 | 24 |
  | E | 5/5 | 0 | 632 | 29 |

  採らない理由は 3 つである。腕 A は 24 行のうち 17〜18 行が直らず、新しい数（loopback-2 の「1000円」）と途中で切れた訳（loopback-4 と 23 の「…が、1」）を出した。腕 E は数を作る行を 5 行まで減らしたが、en→ja の 17 行のうち 11 行で label の後の訳が英語に崩れており、数が消えたのは言語が替わったからである。名指しした 2 行（loopback-11 の「100%具体化」と mic-25 の `"Waves of 1000 Feet"`）は、A でも E でも直らなかった。
  `temperature` 0 でも出力は決まらない。同じ 38 行を 2 回打って出力が違った行は、腕 0 で 4、A で 6、E0 で 14、E で 9 だった（3.6 の訳の依頼の形）。
  だから T と E の両方が数で止まった行は、評議の判定どおり held のまま訳を出さず、原文の字幕を出し続ける。漢数字の数、T に算用数字が無い行、指示の 1 文を日本語で書いた形は打っていない（NOT VERIFIED）。同じ形が全体で何行在るかは、この 2 run の外では数えていない（NOT VERIFIED）。機械が拾うのは、RP-42 の `translate.line.held` で T と E の両方の理由が `number` の行の数である。
- **H67** 認識が session の言語の文字を持たない 1〜2 字の確定文を出し、T がそれを訳さずに断りの文を返した。観測の run の obs2 の `loopback-15`（`--lang en`）の `final` は `ل` と空白 4 つで、`tier` は `omni`、`decode_ms` は 4073 ms である。en の session でなぜ Omnilingual の tier で解いたかは確かめていない（NOT VERIFIED）。直しは 8 の R3e（確定文を出す所で捨てる）で、`omni` で解いた理由は同じ席が振り分けの log で読む。機械が拾うのは RP-65 と、`asr.final.dropped` の数である。
- **H62** 6 席の申告の足し算（a 113、b 14、c 54、材料無し 4、割れる 2）と 8 の「RP-19 の未実装 187 行の仕分けと slice」の表（a 111、b 15、c 57、材料 4）は、置き方の違う行が在る。Python の file ごとに照らすと、違うのは R12d と R12g だけである。R12d は `production_over_splits_relative_to_eval_replica_on_a_skipped_vad_segment` を席が (a)、表が評議へ（Refiner）に置いた。R12g は取り込みの (b) が席で 3 本、表で 4 本で、割れた 2 行も R12g に在る。どの行かは名指しが届いておらず、照らしていない（NOT VERIFIED）。表が正である。持ち主は T の席で、5.3 へ移す時に 6 席の返しと表を行ごとに照らし、違う行を名指しして設計の席へ返す。
- **H63** (a) の 111 行は、その行の module に同じ振る舞いの code が在ることまでを読んで決め、関数ごとの一致は確かめていない（NOT VERIFIED。とくに `put_latest_keeps_only_newest_bounded_items` は同じ名前の関数が無い）。数えた式は `fn` の名前を見るので、test でない同じ名前の関数も在ると数える。持ち主は R12b〜R12g の実装の席で、同じ振る舞いが無ければその行を (c) として設計の席へ返し、未実装の行として残す。
- **H68** 3.17 の効きの見積り（ja の `d` で約 400 ms、en で約 180 ms）は、threshold 0.6 と 0.7 の probe の終わりの差から終わりの線 0.50 の値を読み、無音の差を足した見込みで、確かめていない（NOT VERIFIED）。終わりの線 0.50 は 0.6（線 0.45）と 0.7（線 0.55）の間で、どちらも ja の差の p50 が 288 ms だった。`--min-silence` 0.20 の run と 0.30 の run の `d` と `L` の縮み（3.17）は、負荷を揃えて交互に流した対ではないので、量は確かめていない。完結で確定する標本の割合も分からない。持ち主は R8v の手番 1 の probe で、そこで止まる条件は 8 の R8v の行に在る。
- **H69** R8v だけでは `L` の p95 は 1000 ms に届かない（評議の判定、GO）。判定の中身は 3 つである。1 つ目に、RP-66 の遅れの合否は `d` だけにし、`L`、復号、訳は p95 を記録するだけにする。2 つ目に、`L` の p95 を 1000 ms 以下にする合否は別の受入 RP-67 に置き、持ち主を ja と en に分ける（6 の RP-67 の行）。3 つ目に、VAD だけで届かない根拠を次のとおり置く。ja は、`L` が `d`、復号、訳の足し算で、どれも 0 以上なので、`L` の p95 は訳の p95（1034 ms、e2e1 の n=93）を下回れない。en は、n=70 の nearest-rank の p95 は 67 番目なので 1000 ms を超えてよい標本は 3 件までで、上位の 3 件（4681、1658、1603 ms）がその枠を使い切っており、残りの 29 件（1000 ms を超える 32 件のうち）をすべて 1000 ms 以下にする必要がある。`--min-silence` 0.20 の run で `d` の縮みの約 4 分の 1 しか `L` の p95 に出なかったこと（3.17）も、`d` だけでは足りないことと合う。
- **H70** RP-66 は、3.1 の終端不正の式の `min_silence` を 0.12 秒で数える（評議の判定、GO。3.1 の例外として書いた）。RP-67 にも同じ線を当てる（評議の判定、GO。S1〜S4 の条件を掛ける）。早い規則で確定した標本は `L_asr − L_dec` が 0.30 秒より短くなりうるので、0.30 秒で数えると速く確定した標本ほど有効から外れ、`d` と `L` の p95 が実際より悪く出るからである。線で偏るのは割れの数のほうで、割れた標本が有効から外れると割れの数が小さく出るので、割れた標本と「final なし」の標本は枠の全標本で数え、`L` を数えない訳の取れない有効な標本の数も記録する（3.17 の「RP-66 と RP-67 の数え方」の 3）。値は結果を見る前に決めた。`t_end` の定義と manifest の `end_sample` は変えない。ただし、早い規則で確定しなかった標本にも 0.12 秒を当てるので、`t_end` が声の終わりより後ろに置かれた標本（3.1）を 0.30 秒の時より多く有効に入れる。その数は RP-66 の test が 0.30 秒と 0.12 秒の両方で数えて記録する。
- **H71** 完結の判定の表（3.17 の 3）は ASSUMPTION である。句読点の model が、途中で切れた文の終わりにも `。` を付けるかは測っていない。en の認識器が途中で切れた文に `.` を付けるかも測っていない。R8v の手番 1 の probe が、完結と判定した候補が発話の窓の中に落ちる数で確かめる。持ち主は R8v で、表の行は test-designer の席が case 行で固める（セラが運ぶ）。
- **H72** E2E-2 の記録の en の CER は、run の全標本の和で 988,860（百万分率）と読め、品質の基準に使えない（`/home/fukugan/.cache/hayamimi-e2e-20261015/run3/e2e2.jsonl` を `jq` で数えた。2026-10-15 14:1x）。参照の文の結び違いか採点の不具合かは確かめていない（NOT VERIFIED）。そのため RP-66 の en の品質は、割れた標本の数と「final なし」の数だけで見る。持ち主は E2E-2 の試験の席である。
- **H73** `hayamimi-eval` の DER の run（`der_run.rs` の `Vad::open`）と scorecard も同じ区切りを通るので、R8v の後に RP-1、RP-2、RP-15 の値が動きうる。持ち主は R8v の手番 3 で、悪化したら評議へ戻す。
- **H74** 完結でない候補で解く 1 回は `feed` の中で同期に走るので、その間 window の取り込みが待つ（3.16 の 3 の待ちの log が `hayamimi` に出る）。候補が 1 発話に何回出るかと、速報の復号と core を取り合う量は測っていない（NOT VERIFIED）。持ち主は R8v の手番 1（候補の数）と手番 3（待ちの log）である。
- **H75** 流れの途中の `flush` は、R8v の前から今の main の経路に在る。`Engine::finish`（`native/hayamimi-asr/src/pipeline.rs:618-627`、`flush` を打つのは `:623` の 1 か所）を、`hayamimi` の session が流れの終わり（`native/hayamimi/src/session.rs:113`）のほかに、音声の列の `AudioItem::Flush`（`:108`）と reset（`:297`）でも呼び、その後も同じ detector に feed を続ける。3.17 の方針 4 の機構どおりなら、声の途中で `Flush` が来てその後に無音が続くと、今の main でも process が abort しうる（NOT VERIFIED。live の run で起きた記録は探していない）。直しは R8v の手番 2 の最初に入れる（`finish` が `flush` と segment の取り出しの後に detector を作り直して offset を進める）。RP-68 の子 3 が守る。持ち主は R8v の手番 2 の席である。
- **H76** held-out（8 の R8v の手番 1）の ja の除外の 19 本（`lat-ja` の golden と同じ `id`）は評議の席の数えの転記で、この席は `id` を突き合わせていない（NOT VERIFIED）。手番 1 の席が `manifest.json` の `id` と golden の `id` を突き合わせ、除いた 19 本の `id` を `heldout.md` に書く。en の AMI の turn の規則（1 秒以上、前後 0.3 秒に他の話者が掛からない）で取れる発話の数は数えていない。数が 15 未満なら、手番 1 の席は流さずに評議へ出す。FLEURS の en を取り直す道具（`hayamimi-eval fetch-set --pin` が FLEURS の音声を取れるか）も確かめていない。
- **H77** 今の detector A の segment が 1 つも触れない発話が在る（品質の穴）。held-out の ja で 81 本中 8 本、`lat-ja` で 23 本中 1 本（`heldout.md` と `probe.md`。転記）、AMI で 60 本中 2 本である。その発話は final が出ず、字幕に何も出ない。R8v の B は A の声の中でしか働かないので、R8v はこの穴を直さず、遅れの数え方（3.17 の数え方の 10）でも母数から外している。A が逃す理由（声の大きさ、短さ、`min_speech` 0.25 秒、threshold 0.5）は確かめていない（NOT VERIFIED）。持ち主は R8v と別の席で、手番 1c の held-out でも同じ数を数えて記録し、直すかと直す席は評議が決める。
- **H78** 手番 1c の材料は確かめていない所が 4 つある（NOT VERIFIED）。`hayamimi-eval fetch-set --pin` が `japanese-asr/ja_asr.reazonspeech_test` の行を取れるか。`testdata/eval_real` の ja の manifest は行の番号を持たないので、除外を参照の文の一致で行うこと（同じ文の別の行も除く）。AMI の残りの turn の数（手番 1 の 5 会議 × 12 本の外）。ReazonSpeech の頁に許諾の記載が無いこと（S32。評価の材料として git の外に置き、配る物に入れない）。どれも手番 1c の席が流す前に数え、ja が 80 本、en が 30 本に満たなければ流さずに評議へ出す。
- **H79** en の割れの仕分けは、参照の文の終わりの印を A の final の句読点から取る（3.17 の数え方の 9。ASSUMPTION）。物差しにする A の final が、文の途中に句点を打つ（認識器が本物の切れ目でない所に句を打つ）と、その位置の割れを本物の文の切れ目と仕分けるので、文の途中の切断を見落とす向きに偏る。ja の 13 件では、文の途中は参照の仕分けで 6 件、A の final の仕分けで 1 件だった（参照で文の途中の 6 件は A の final では切れ目と読まれた。出所は `1b/r8v_heldout_ja_probe.txt` の 117〜119、122、123 行と `1b/r8v_latency_set_probe.txt` の 44 行。A の final で文の途中で参照では切れ目の 1 件は同 42 行）。偏りの量は ja の 2 通りの仕分けの一致の数で見る。
- **H80** 部分訳の使い回し（3.18、R8w の 2）の対象は、今の記録で確定文の 52/125 だけで、RP-32 の遅い 5 発話のうち 4 と 23 にしか効かない見込みである（`docs/results/rp32/` の 3 run を `jq` で数えた。発話への結び方は `rs1.jsonl` の `finals` の数で順に結んだ近似で、`t1_s` で結んでいない。NOT VERIFIED）。8 と 13 は速報が ITN の前の文字列で、22 は確定文の語が速報より多い。Python 以下に反転する見込みの発話は 23 だけで（4 は T が V に落ちる行が 15/15）、使い回しだけでは対の比較は 15/19 に留まる見込みである。速報に ITN と句読点を掛けて対象を広げる形は採らない（評議の判定）。17 に届かない時の次の手は 2 つである。1 つ目は 3.17（R8v）で `d` を縮めることで、反転に足りない量は発話 4 が約 240 ms、12 が 71 ms、22 が 202 ms である（転記）。2 つ目は 3.16 の 2 の B で、T が V に落ちた行の `hold_reason` を数え、正しい訳を止めている理由だけを直すことである。`docs/results/rp32/` には中継の stdout が無いので、次の RP-32 の Rust の run で 3.16 の 4 の手順どおり残す。持ち主は R8w の 4 の run の席で、run の後に発話ごとの `L` の中央値、`translate.draft.reused` の数、`hold_reason` ごとの行の数を並べる。速報の答えが確定文の着く前に出ている割合（`state` が `done` の行の割合）も、同じ run で数える。
- **H81** A（T と E0 を並べて投げる形）は 3.16 の 2 の作る条件から外した（評議の判定）。根拠の E0 の p50 640 ms は H66 の probe の 76 本（腕ごとに 2 回、負荷の記録なし）の値で、RP-32 の run の負荷の下の値ではない（NOT VERIFIED）。開き直す条件は、E0 の 1 回の p50 が E の 1 回の p50 を下回った時である。その値を測る run は今は無く、E0 の依頼の形を変えた席が測って設計の席へ返す。
- **H82** 推測デコードの効き（3.18 の式の `α`）は、この model の訳の依頼では測っていない（NOT VERIFIED）。n-gram の下書きは既定の値（照らす並び 12 token と 24 token、最小 48 token）が T と E の答え（19〜45 token）より長いので腕に入れていない。小さい値で効くかは測っていない。腕 S3 の語彙の一致は、`tokenizer.model` の sha256 の一致と、llama-server の起動の log で確かめる（3.18 の推測デコードの決めの 3）。`tokenizer.model` の sha256 は評議の数えの転記で、この席は 2 つの file を開いていない（NOT VERIFIED）。llama-server が下書きの model の語彙の食い違いを起動の時に log へ出すかと、その行の文面（`draft acceptance rate`、`#acc tokens` を含む）も、llama.cpp の版で確かめていない（NOT VERIFIED。RP-71 を流す席が `--help` と同じ手番で記録する）。RP-71 の run は `eval` の 1 行ずつの直列の依頼で、slot 4 本が同時に埋まった時の推測デコードの効きは RP-32 の run でしか見えない。持ち主は R8w の 3 の席（probe）と 4 の席（RP-32 の run）である。
- **H83** `/ingest` の 8766 が Python を止めて 21 秒と 85 秒の後に取れず、166〜170 秒の後に取れた（原因の席の申告の転記）。原因が Windows の TIME_WAIT かは確かめていない（NOT VERIFIED）。3.18 の手順は、待ち受けと接続の両方が消えるまで待つので、TIME_WAIT でも別の process でも同じに扱うが、別の常駐物が 8766 を持ち続けると run が始まらない。待った秒数は `ports<N>.txt` に残る。RP-66 の main と R8v の交互の run（3.17）も同じ形で port が移りうるが、RP-72 は RP-32 の dir だけを見る。持ち主は R8w の 1 の実装の席で、RP-66 の run の dir に `events.jsonl` が在るかを確かめ、在れば同じ判定を RP-66 の読み口に足すかを設計の席へ返す。8080 の llama-server は別の app が起こし止める共有の unit で、RP-71 の腕を採るまでの RP-32 の run はその unit に頼る。run の間にその app が server を止めると、訳の取れない標本が増える（NOT VERIFIED）。
- **H84** 3.1 は、最後の速報の原文が `final` と同じ文字列なら、その速報の訳を `t_tr` に入れると書く（3.1 の「速報の原文が最後の速報で `final` と同じ文字列になっていれば」の文）。`hayamimi-eval` の `latency.rs:94` は、`of` が `partial` の訳を文字列に依らず `t_tr` の候補から外し、T の T-RP-31-010 もその形を固定している。3.18 の使い回しは `of` が `final` の先の訳を出すので、この食い違いの影響を受けない。どちらに揃えるかは設計の席が決め、揃えた側の test の行は test-designer の席が書き直す。今の記録では速報の文字列が `final` とそのまま同じ行は 0 で（3.18 の数えた結果）、どちらに揃えても今の値は動かない。
- **H85** RP-70 の比べは「以上」と「以下」で、幅を置いていない。同じ依頼を 2 回打っても出力の違う行が在る（H66 の 38 行中 4）ので、a と b に差が無くても、どちらかの向きに偶然で割れうる。2 回ずつ合わせて比べるのはそのためで、割れの大きさは a 同士の差（RP-70 の陽性対照）で読む。RP-70 が落ちた時は、3.18 の部分訳の使い回しのとおり鍵を「同じ」だけに狭め、a 同士の差と a と b の差を並べて記録する。dev の seed 0 と seed 1 の組は V の調整に使った組で、held-out ではない。a と b の 2 つの腕の比べにだけ使い、記録の値（RB-7、RP-13）と比べない（RP-37 と同じ扱い）。持ち主は RP-70 を流す E2E の席である。
- **H86** RP-32 の行ごとの有効の線（3.1 の例外）で、全標本を `min_silence` で数えた時と値が変わるのは、`rule` が `early` の Rust の標本だけである。対の比較の余裕（行ごとの線で数えた数 − 閾値）は小さいことがあり、その時は 1 発話の入れ替わりで合否が変わるので、報告に余裕を数で書く（3.16 の 4 の「RP-32 の報告」）。`rule` が `max_speech` と `flush` の標本を `min_silence` で数えるのは、線を下げる規則を早い規則で区切った標本だけに限るこの席の決めで、評議の判定は `early` と `silence` を名指した（ASSUMPTION。`max_speech` と `flush` の標本の数は結果に `rule` ごとに書くので、その数で読む）。負荷の差の向き（3.16 の 4）は、load が `L` を延ばす向きに効くこと（3.5 の R7 の測り直しの読み）に頼り、`L` が load とともに単調に延びるかは測っていない（ASSUMPTION）。Rust の run の前の load が基準の上端 0.83 より低い run は、負荷の差が Rust に有利な向きに効いた疑いとして結果に名前と値が残る。持ち主は R8h の 4 の run の席で、余裕が 0 以下か、0.83 より低い run で通った時は、値を添えて評議へ出す。T の 4.42 の行のうち、流した順を見る T-RP-32-001〜004 と 029 と、`hayamimi` の stdout の無い今の dir を読む T-RP-32-045 は、この形に合わせて test-designer の席が書き直す（セラが運ぶ）。受入 test の名前 `rp32_alternated_runs_under_the_same_load_beat_python` は code に在る名前なので変えず、意味は 6 の RP-32 の行で読む。
- **H87** Python の基準の 3 run の GPU の読み（`stream` が `--out` の隣に書く `<run>.gpu.json`）は `docs/results/rp32/` に無い（`git ls-files docs/results/rp32/` は jsonl と `load<N>.txt` だけを返す）。そのため 3.16 の 3 の GPU の読みの対の規則と RP-34 の無効にする対の数は、基準の run の側の読みが無いまま組めない。git の外の run の作業 dir に `py<N>.gpu.json` が残っているかは確かめていない（NOT VERIFIED）。持ち主は R8h の 4 の run の席で、残っていれば同じ dir へ写し、無ければ RP-34 の対の規則をどうするかを評議へ出す。
- **H88** E2E-1、E2E-2、RP-3 は有効の判定に全標本で `min_silence` を渡す（8 の「端から端の受入」）。R8v の build（既定の `--endpoint early`）では、早い規則で確定し `L_asr − L_dec` が `min_silence` より短い標本がこの 3 つで有効から外れ、速く確定した標本ほど母数から抜ける。RP-32 の行ごとの線をこの 3 つにも当てるかは受入の定義を変える判断で、この席は決めない（評議の判定が要る）。持ち主は評議（セラが運ぶ）である。
- **H89** RP-61（8 の「清書（Refiner）を live に足す形」の受入と 6 の RP-61 の行）は、清書を入れた Rust を「同じ負荷で交互に測った Python」と比べる。RP-32 の基準を固定して Python の run を打たない形と食い違う。基準の `py1`〜`py3` が清書を既定のまま入れた run かは確かめていない（NOT VERIFIED）。RP-61 を固定した基準と比べる形に直すかは評議の判定が要る。持ち主は R12w4 の席で、run の前に評議へ出す。
- **H64** RP-61 の許す悪化の幅（清書を切った Rust に対して p50 で 30 ms、p95 で 60 ms）は実測の裏づけが無い仮の値である（NOT VERIFIED）。清書の別の `Router` が足す host の memory の量も測っていない（NOT VERIFIED）。持ち主は R12w0（W6 の席 1、清書 1 group の重さ）と R12w4（W8 の席 1、RP-61 の run）で、R12w4 の席が幅を置き直し、超えたら評議へ戻す。Refiner の 17 行は R12w0〜R12w4（8 の「清書（Refiner）を live に足す形」の表）が入るまで未実装の一覧に残る。
- **H65** `History::clear` が絶対の位置を戻さない形（R12h）は、Python の `test_audio_history_clear_empties_buffer_and_offsets` の「offsets を空にする」と違う。T の case 行は「buffer が空で、reset の前の sample が preroll に入らない」に合わせる。持ち主は T の席である。
- **H58** R8m の `listen` が本物の中継の event を本物の `/events` から SSE で読むことは打てていない（WSL に出力の装置が無い）。持ち主は E2E の打ち直しの席で、Windows の実機で RP-41 の `--ignored` の行と E2E-2 を打つ。
- **H55** Windows の `hayamimi.exe` が今の main で build されておらず、RP-10（`lang` の条件つき）と Windows の実機の E2E を打てない。持ち主は Windows の build を持つ席（R10 の続き）で、build の後に、書いていない検証の席が RP-10 を打つ。
- **H51** V の数の検査は、ko の語だけで書いた数（`오백만`）と、es の小数点が読点の数（`1,5 millones`）を読まない。原文と同じ数の正しい訳が `number` で止まりうる。es の `1,5` は区切りの読点と見分けられない。持ち主は R8l の後に `numbers.rs` を触る席で、dev の組の vprobe で `number` で止まった参照訳の行を数え、語だけの数が何行在るかを材料に、直すかを評議へ出す。
- **H52** `native/hayamimi-relay/tests/cs28.rs` の 2 本が、全体の run の 1 回目に 1 回だけ落ちた（`log held 0 of 1`、30.07 秒）。cs28 だけを打つと 3 本とも通り、全体の 2 回目も FAILED 0 である。原因は特定していない（NOT VERIFIED）。30 秒は待ちの上限に似た時間で、他の test と並んで走った時に log の行を集め終える前に待ちが切れた疑いがある。持ち主は次に中継の送り先か log を触る席で、待ちの間に集めた行の数を失敗の文に出す形にし、全体の run を繰り返し打って落ちなければ、この穴を消す。
- **H50** `native/hayamimi-relay/src/hub.rs` の `every_connected_client_receives_every_event_in_order` は、応答の head を読んだ直後に `hub.clients()` を数えるので、購読の登録より先に数える形の疑いがある（NOT VERIFIED）。load 約 2 で単独 10 回打って落ちていない。持ち主は次に `hub.rs` を触る席で、数える前に登録を待つ形にし、繰り返し打って通ったらこの穴を消す。
- **H46** 3.15 の「llama-server に届かない」の行の、5 秒ごとの `/health` の確かめは作っていない。`live` は依頼の時にだけ届かないことを検出するので、確定文が来ない間は `translate.fallback.held` が出ず、画面の印も遅れる。E2E には要らない（E2E-1 の陰性対照は llama-server を止めて音声を流すので、依頼の時の検出で `translate.fallback.held` が出る）。持ち主は R8h の後に中継を触る席で、E2E の後に作る。受入は RP-29 の R8b の行に、確定文を送らずに llama-server を止め、6 秒以内に `translate.fallback.held` が 1 件出る case を足す（test-designer の席が書く）。
- **H7** 発話の終わり `t_end` は manifest に書いた sample の位置で決める。実放送の音声は参照の境界と本当の発話の終わりがずれることがあり、`L` に数十 ms の誤差が入る（ASSUMPTION）。Python と Rust に同じ誤差が入るので、比較には効かない。
- **H8** RP-3 の en→ja の 2000 ms、RP-7 の 20%、RP-22 の 14,000 MiB は、実測の裏づけが無いまま据え置いた値である（NOT VERIFIED）。lat-en の Python の基準が無く、R1 の `L` の p95 1543 ms と 1890 ms（`gpu_stages.md` の「遅れ L の見積り」の「Rust 認識 cpu / 訳 cuda 無負荷」と「負荷」の行、:307-308）は見積りで、RP-7 の R1 の値は認識を CUDA に置いた構成の値で、`gpu_stages.md` に memory の実測が無い。持ち主は 2000 ms が R8e、20% が R7、14,000 MiB が R3 と R8e で、測って超えたら評議へ戻す。RP-3 と RP-4 の対の比較の下限（比較集合の発話の数 × 20 / 23 の切り上げ）と根拠 3 の 1% は、R0 と R1 の実測で裏づけた（6 の RP-3、3.5）。
- **H8b** RP-28 の CPU に落ちた時の `L` の上限（p50 4500 ms、p95 7000 ms）は、R1 の段の和の見積りに約 1 割を足した仮の値で、実測ではない（NOT VERIFIED）。R8e が測って置き直す。
- **H8c** RP-6 の 256 MiB と 64 MiB は、Windows の 1 回の実測（基準 1198 MiB、最大 1957 MiB）から置いた仮の値である（NOT VERIFIED）。`memory.used` は GPU 全体の値なので、ほかの process が同じ時に memory を取ると誤って通る。RP-6 はほかの GPU の process を止めた機械で打ち、`gpu.session.created` の provider と合わせて読む。R2 が WSL でも同じ読みを取り、R1b と同じ評議で置き直す。
- **H9** `--min-silence` は `L_asr` の最大の部分である。比較の値と Rust の既定は 0.30 で（8 の R0、`baseline_python.md:205-212`）、2 回の run で判定が割れた 0.25 は、R8h の後の席が E2E の後に Rust で 3 回以上流して決める（R7 の席は閉じた）。
- **H10** 確定のとき、最後の速報の解き直しと確定の音声が同じなら、確定の解き直しを省ける見込みがある（ASSUMPTION）。R8h の後の席が E2E の後に、同じ音声かを判定する条件と、省いた時の誤り率を測る（R8h の 3 と同じ loop を触るので、同じ席が続けて打つ）。
- **H11** sherpa-onnx の中の fbank と探索を分けて測る口が無いときは、3.5 の差の式で出すので、値は推定になる。R1 の結果の file にそのことを書く。fbank か探索が遅い段だと分かっても、sherpa-onnx の中を替えるのは C++ を書く変更になるので、評議へ出す。
- **H12** Nsight Systems が Windows の Rust の実行 file を profile できるかは確かめていない（NOT VERIFIED）。できなければ WSL の同じ build で数える（3.5）。
- **H13** Kiwi（ko の分かち書き）の Rust の binding `kiwi-rs` は Kiwi の C の library を要し、LGPL である。今と同じく任意とし、無いときは素通しにする。Windows の msvc の build から読めるかは R3 で確かめる（NOT VERIFIED）。
- **H14** `docs/results/scorecard.md` の評価の音声は手元に無い（`testdata/eval_real` が無い）。R0 の `fetch-set` が取り直す。取り直した音声が scorecard.md の時と同じかは、R0 の Python の値が scorecard.md と 0.005 以内で一致すること（RB-3）で見る。
- **H16** 英語版 `rust_port.md` は R10 の成果物である。それまでこの文書の日本語だけが在る。
- **H17** CAT-Translate-3.3b の GGUF は配布が無く、会話字幕の試しでは自分で変換した（会話字幕の設計 2.3）。変換は Python の道具（llama.cpp の `convert_hf_to_gguf.py`）を使う。訳の model を決める会話字幕の設計 9 の Q1 が CAT を選んだときに、変換を Python 以外で行う手段が無ければ、その 1 回の変換を規約の例外とするかを評議へ出す。3.18 の腕 S3 の下書きの model（`CAT-Translate-0.8b`）は、公開の GGUF（S37）を使うので変換せず、この例外は要らない（評議の判定）。
- **H18** 別の席の調べの「GPU の方が遅い」「GPU の道具が無い」は推測を含む転記で、この文書は根拠に使っていない。3.5 の根拠は R1 の実測と一次資料である。
- **H19** 退路の ORT 1.28.3 は両 OS とも Microsoft の asset である。WSL の build は archive の sha256 が GitHub の digest と一致し、RP-17 を通した（実測、2026-10-14）。Windows の 1.28.3 の build は作っていない（NOT VERIFIED）。退路へ戻すときは Windows でも作って RP-17 を通し、RP-24 で 2 つの OS の版の名前をそろえて記録する。
- **H20** hooks の `guard_commands.rs` の llama.cpp の鮮度の検査は、`llama-` で始まる名前を含む command を、WSL の build の木の鮮度で判定する。Windows の llama-server を WSL の shell から起こすと、関係の無い木で refuse される。Windows の llama-server は PowerShell から起こすと決めた（3.6）が、検査の対象を名前でなく path で分ける直しが要るかは、R8b の手番で hook-engineer の席へ出す。
- **H24** `ferrous-opencc` の t2s が、Python の OpenCC 1.4.2 の t2s と同じ文字列を出すかは確かめていない（NOT VERIFIED）。R3 で、R0 の yue の参照文と認識文の全件を両方に通し、差の行の数を数える。差が在れば、yue の CER の差がその行で説明できるかを RP-1 の結果に書く。
- **H22** `state.json` の `constraints` に「Rust への移植は速度が足りないと測れたときだけ」が残っていて、この文書の目標と食い違う。state.json を書くセラが直す。
- **H23** 速報の訳が `final` と同じ原文になったときの `L_tr` の負の値は、速報を早く出す形の正しい効果だが、その発話の数が多いと `L` が小さく見える。RP-3 の出力でその数を見て、多ければ R1b で読み方を決める。
- **H25** Python の `asr_engine.py` の 1715 行目は、早めに判定した言語を 4 秒未満の発話にだけ使い、それより長い発話では確定のたびに言語判定をやり直す。やり直し 1 回あたり 114 ms で、`L_asr` に入る。Rust でこの規則を写すか変えるかは、R1 の言語判定の GPU と CPU の実測（3.5）の後に R3 が決める。
- **H26** `end_sample` の今の規則（20 ms の frame の音量）は、声の後ろに音楽が続く発話（lat-ja の 18 と 19）で声の終わりを決められず、その標本は終端不正に入る。全発話に当てる別の規則（Silero VAD の区切り、手で付けたラベル）で直すかは決めていない（NOT VERIFIED）。発話 18 と 19 は Python の訳が 1 回も取れないので、`L` の比較集合には入らない。直すなら 3.1 の規則のとおり全発話を置き直し、Python の固定した記録（py1〜py3）を、新しい `end_sample` で数え直す。Python は流さない。数え直した値には、H42 と H43 の未検証を添える。発話 18 と 19 は 3.1 の起点の確かさの組分けで不確かと切れに入り、声の本当の終わりの扱いは H91 に在る。
- **H27** 機械語の当たらない kernel を driver が PTX から変換する初回の遅れは測っていない（NOT VERIFIED）。R1 の RP-23 で、JIT の cache が空の run（子の process に `CUDA_CACHE_PATH` で空の directory を渡す。S18）と、同じ cache で続けて流した温かい run の `decode_ms` を別の行に記録する。差の読み方は R1b で決める。
- **H28** Windows で `onnxruntime.dll` の import が System32 の 1.17.1 に結びつく条件は特定していない（NOT VERIFIED）。3.4 の遅延読込みと先読みは probe の 4 回で防げたことだけを確かめている。test の実行 file は cargo が `target/<triple>/<profile>/deps/` に置き、`sherpa-onnx-sys` が DLL を写す `target/<triple>/<profile>/` と directory が違うので、test の process で「実行 file の隣」を先読みしたときに `onnxruntime.dll` が在るかも確かめていない（NOT VERIFIED）。R1 の席が `rustc-link-arg-tests` を足す手番で、test の process の `modules` の origin を数える。
- **H29** 3.5 の「負荷の下」の値は、WSL の llama-server と Windows の ORT が同じ GPU を分けた値で、本番の分け方（Windows の llama-server と Windows の ORT）ではない（NOT VERIFIED）。訳 T の GPU の値は `sm_120` を持たない CUDA 12.0 の build で測ったので、悪い向きに偏りうる（3.12）。R8b が Windows の CUDA 13.4 で T を測り直し、R8e が本番の分け方の負荷の下で `L` を測る（RP-3）。GPU の往復の下限（219 µs）も R1 の計測の値で、測り直していない。
- **H30** 認識の fp16 の GPU が fp32 の 2.7 倍遅い理由は未特定である（NOT VERIFIED）。`ort` の session で CUDA の時に CPU に置かれる node は fp32 が 1599、fp16 が 1611 で（`gpu_stages.md` の node の表の `encoder-epoch-35-avg-1.onnx` と `encoder-epoch-35-avg-1.fp16.onnx` の行、:246 と :245）、差の 12 node では説明できない。認識は CPU に置くので置き場には効かないが、GPU で fp16 を選ぶ段が他に出たときに同じ形で遅くなりうる。
- **H31** 認識全体の cuda の時間（同じ fp32 の無負荷の p50 171414 µs）のうち、encoder だけの時間（66262 µs）と fbank（約 6 ms）を除いた約 99 ms が何に使われているか説明できない（NOT VERIFIED）。転送は 1 発話あたり host → device 1524 µs と device → host 935 µs で（`gpu_stages.md` の転送の表の fp32 の 2 行、:221-222）、この差を説明しない。node の置き場は `ort` の session の値で、sherpa-onnx の session の置き場は確かめていない。認識は CPU（int8）に置き（3.5 の表の後の段落）、この時間は置き場に効かないので、今は調べない。認識を GPU へ移すかを評議が開く時に、その席が内訳を調べ、encoder と探索を別の provider に分けずに縮める手段が在るかを評議へ出す。
- **H32** llama.cpp の CUDA の build が CUDA の device 0 の機械で起動し CPU で動くか、`-ngl 0` で cuDNN と cuBLAS を読まずに済むかは確かめていない（NOT VERIFIED）。R8b が `CUDA_VISIBLE_DEVICES` を空にして起こして確かめ、起動できなければ CPU の build（`llama-<tag>-bin-win-cpu-x64.zip`）を同じ置き場に並べる。sherpa-onnx が CUDA の実行 provider を作れないときに自分で CPU へ落とすかも確かめていないので、3.11 の 2 段目は `gpu.session.created` の実際の provider で判定する。
- **H35** RP-1 の誤り率の差は、Rust の code ではなく ORT の build の違いから出ている（R3c の実測。Python の wheel の ORT 1.27.1 を Rust に読ませると 66 clip が Python の log と小数 3 桁で一致し、自分で build した ORT では 5 clip が違う）。Windows の値（ja、en、zh、ko、yue の百万分率）は、Python と同じ library で 37512、22606、65609、80572、61454、配る形の自分の build で 37512、22606、65609、86939、61454 である。RP-1 の小数 3 桁の比べ方（6 の RP-1）では、配る形の build の ko の 0.087 だけが Python の 0.081 を超える。ORT の版は古くしない（常に最新の版を使う）。RP-1 の閾値も緩めず、ko の行は赤のままこの穴に残す（評議の判定）。Rust の code は基準（`testdata/baseline/scorecard_run1.md` の ko の行）と同じ物を出す。Python の wheel の ORT を読ませた run（`win_python_libs.json`）で ko が 80572（0.081）、66 clip が一致したからである。R11 の入口では ko の上限を配る形の build の記録値 86939（百万分率、丸めずに整数のまま比べる）にする（3.2 の 2）。SenseVoice を CPU の fp32 で読む形は採らない。`L` の延びが 1% 以内という条件を満たさないからである。fp32 の ko の 0.0937 と yue の 0.0912 は、VAD が欠けて長い音声を一定の長さで切る `model.fallback.chunks`（`decode.rs` の 416〜430 行）が入った値で、精度の効果とは読めないので引かない。R3b と R3c は ko をこの穴として残したまま merge してよく、E2E は ja と en だけを使うので止まらない。振り分けを通した Windows の配る形の build の run（`lid: detected`、記録は R3c の `docs/results/r3c/win_routed.json` と `h35_clips.md`）でも、ja 37512（LID 15/15、ja_02 は誤り率 0）、en 22606、zh 65609、ko 86939、yue 61454 で、正解の言語を渡した run と同じ値になり、言語判定の外れは 66 clip 中 0 である。zh と yue は Python と同じ値で、RP-1 を通る（R3c の検証の席の可、`b8fd07e`）。R3b の run の zh 0.0699 と yue 0.0682 は、この build では出ない。`hayamimi-eval` の `cli.rs` の先読みの 4 行が Windows の target で compile されることと、T-RP-1-013 の実記録は確かめていない（NOT VERIFIED。持ち主は E2E の席）。
- **H38** 日本語の streaming の認識器が出ている（`docs/research/04-recheck-2026-10.ja.md` の「速度と品質に効きそうな変化」）。今の認識は非 streaming の認識器を VAD の区切りに掛け、`--min-silence` の無音を待ってから解くので、`L_asr` に無音の待ちと発話全体の解き直しが入る。streaming の認識器は、話している間に解き進めるので、`L` を縮められる見込みがある（NOT VERIFIED）。`goals.md` の 56 行目の「高精度な日本語の streaming model が出たら増分デコードを再評価する」の条件に当たる。候補は 3 つで、Rust の実行時の経路に載るかが違う。Nemotron 3.5 ASR Streaming 0.6B（ja-JP を含み、chunk は 80〜1120 ms、FLEURS の日本語 CER は 80 ms で 13.87、1.12 秒で 11.48）は sherpa-onnx が runtime を取り込んでおり、今の build の `OnlineRecognizer` で測れる見込みである（取り込みが v1.13.8 に入っているかは確かめていない）。Moonshine v0.1.5 の日本語 streaming と Voxtral Realtime（sub-200 ms から設定可）は、sherpa-onnx で動くかを確かめていない。Python や vLLM でしか動かない物は、実行時の Python を 0 本にする方針（1.3）なので採らない。持ち主は R7b（W3 の席。測るだけで、`hayamimi` の経路の code を替えない）で、Nemotron 3.5 の 560 ms の int8 の package を同じ build で読み、lat-ja の `L_asr` の p50 と p95（RP-4 の式）と、eval_real の ja の CER（RP-1 の ja）と golden の 8 本の CER（RP-2）を、今の ReazonSpeech と同じ表に並べて `docs/results/` に書く。Moonshine と Voxtral は、sherpa-onnx の docs と release に在るかだけを確かめて同じ表に書く。置き換えるかは、E2E が通った後に、その表を材料にして評議が決める。いまの W1〜W4 と E2E の経路は替えない。Nemotron 3.5 の license（OpenMDW-1.1）を配る物に入れてよいかは確かめていない（NOT VERIFIED。採る時に R10 が `THIRD_PARTY_NOTICES.md` の手番で確かめる）。
- **H39** 道具の版の差が 3 件ある（同じ文書の表 E）。llama.cpp は b11583 から b11609 へ進んだ。持ち主は R8e で、L を測る前に R8b の置き場の build を b11609 以降の最新に入れ替え、前の版を控えて RP-16 の訳の行と CS-20 を打ち直す。ORT の退路は同じ系列の 1.28.3 にした（3.12）。WSL で退路の build を 1 回作り、RP-17 を通した（6 本 pass、実測 2026-10-14）。xtask の `--ort-version` に退路の既定は無く（既定は現行の 1.31.0）、xtask は変えない。Windows の退路の build は未作成で、持ち主は R10 の続きである（NOT VERIFIED）。現行の 1.31.0 は最新のままである。`requirements.txt` の sherpa-onnx 1.13.6 は上げない。R0 の Python の基準をその版で測ったので、上げると基準を同じ条件で取り直せなくなる。この file は R11 で消す。
- **H41** 同じ process で、sherpa-onnx の CUDA の認識（recognize）の後に `ort` の CUDA で encoder だけの session を開くと、`ort` が落ちる（R7 の計測の道具の実測）。原因は特定していない（NOT VERIFIED）。計測は 2 つを別の process で取る形にした。本番の process も sherpa-onnx の CUDA の session（言語判定）と `ort` の CUDA の session（句読点）を同じ process に開くが、その組は RP-6、RP-16 の実走（T-RP-16-028 の `Engine::open`）で落ちていない。落ちるのが同じ model の file を 2 つの runtime で開く時だけかを、R8h の後の席が E2E の後に 1 回確かめる（W4 の測り直しの窓では打たなかった）（同じ process で、言語判定の CUDA の session の後に句読点の session を開く順と、認識の後に encoder を開く順を 1 回ずつ）。本番の組で落ちれば、3.15 の止めない形に反するので評議へ出す。
- **H36** H36 は開いたまま。held-out で V は参照訳 200 行中 47 を止め（直す前 52）、T の訳 53 行（同 52）を止める。うち 31 行は正しい参照訳も止まる行で、壊した行の止まる率は数 33/33・否定 29/33・推量 12/17。推量の表で止まりが増えたので zh/ko/es では推量を止める理由から外してログに残し、残る穴の ja/en の E 待ち（速度）は持ち主の R8h の 2 の B に回す（評議の判定）。
zh、ko、es の否定の検査は原文の否定の語（`ja.negation`）の有無の一致だけを見るので、原文に語が無い行で訳の否定が落ちても止まらない。dev の vprobe の否定の壊し行で止まらない行は zh 5/13 と 2/20、ko 1/13 と 1/16、es 1/10 と 0/18 で、全部 原文に語が無い行である。RP-37 の zh seed 0 の否定 9→8（id 1512：main では推量の偶然で止まっていた）はこの穴の免除として扱う（評議の判定。持ち主は R8h の 2 の B の席）。
zh、ko、es の否定と推量の語の表は `native/hayamimi-relay/data/verify_markers.toml` に在り、推量の側は R8i で止める理由から外す（3.6）。held-out の数は、直す前と後で、壊した行の止まる率が数 33/33 から 33/33、否定 28/32 から 29/33、推量 9/17 から 12/17 である（検証の席の申告を評議が log から打ち直して一致）。
V は止めすぎてもいる（RP-13 の run で、正しい訳を止めた行が RP-13 の chrF を下げる）。止めすぎの直しも R8f が持ち、条件は 3 つである（評議の判定）。調整は seed 0 の 50 組とは別の組で行い、seed 0 の 50 組は判定にだけ使う。否定、数、推量を落とした訳が止まる対照の case 行は残す。閾値は緩めない。含意の否定（否定の語を持たない否定の言い方）と伝聞の「そうです」の扱いは、V の規則そのものを変えるので、その数（止めた行と止めなかった行）を添えて評議へ戻す。RP-13 は R11 に入る条件（3.2）に入る。
V の直しの参照訳の止まる行 70→30（seed 0）と 98→55（seed 1）は調整に使った組の値で held-out ではない。held-out の値は FLEURS の test split の組を 1 回だけ使って出した上の数で、test split の組は二度と使わない。RP-13 の扱いは 6 の RP-13 の行に在る。
dev からは新しい組を引けない。dev の ja と対象の言語の重なりは en、zh、ko、es で 135、135、118、135 行で、seed 0 の 50 組を引くと 85、85、68、85 行になり、seed 1 の組と同じ行しか残らないからである。
test split は 321、320、249、320 行で、dev と id の重なりは 0 である。
test split の組は試験の席が引いた（16 file、800 行。置き場は直す席に渡さないので、この文書に書かず state.json に置く）。ids の sha256 の頭 16 字は en `980017baf068f055`、es `e68a64d7cad454db`、ko `af6b2cf34bc5d823`、zh `c25ea26435365121` で、dev との id の重なりは 0 である。
V の効果を測る壊した訳の組（`vprobe` の (b)）は、参照訳の数を 1 字ずらす、否定の標識の語を落とす、推量の標識の語を落とす、の 3 つの壊し方で作り、V がその行を止めることを見る。
`fleurs-pairs --split <dev|test>` が、表を引く split を選ぶ（`native/hayamimi-eval/src/fleurs.rs` の `tsv_url`）。
既定は dev で、既存の run の組（`testdata/baseline/pairs`）と byte で一致する。
dev と test 以外の値は使い方の誤り（exit 2）で止まる。
test split の表は `https://huggingface.co/datasets/google/fleurs/resolve/main/data/<config>/test.tsv` にある（T-RP-13-022〜026）。
held-out の測りに使う道具は `hayamimi-relay vprobe --pairs <dir> --lang <en|zh|ko|es> [--out <dir>]` である。
出す物は、参照訳をそのまま V に通して止まった行 id、参照訳を決まった規則で壊した版（数を 1 字ずらす、否定の標識を落とす、推量の標識を落とす）を V に通して止まった行 id（壊し方ごと、壊した行の数つき）、`--out` を付けたときの T だけの訳（`noV/` は V なし、`V/` は止めた行を空にした file）である。
V の公開の入口（`Verifier::open`、`verify_into`）だけを使う。R8i で `verify_into` の引数が増えたので、R8i より前の木には `git apply` でそのまま入らない。前の木で打つときは、その木の `verify_into` の形に合わせた写しを使う。
seed 0 の 50 組（dev）の参照訳は、直す前の木で 70 行（en 14、zh 20、ko 14、es 22）、直した後の木で 30 行（en 3、zh 11、ko 8、es 8）止まり、V の直しの前後の数と一致した（T-RP-13-027〜031）。
組を引くのは test-designer の席で、打つのは V を直していない検証の席である。直す席には組の path を渡さず、組は 1 回打ったら使わない（test split の組は 1 回打ち済みで、以後は使わない）。
品質の記録（RB-7 の値）は書き換えない。
E を掛けない腕で出た en の 46.65 は RP-13 の en の値ではなく、RP-13 の en は未測定である。
V の直しは `d888f87` の 1 commit で、revert で戻せる。
T-RP-13-012、013、016、017、018 は、ja と en の外の言語で E を掛けず、V で外れた行を `held` にする形に合わせて書き替える。書き替えは test-designer の席へ回す（セラが運ぶ）。
- **H33** CPU に落ちた段の誤り率は、同じ model を読むので GPU の run と同じと見込んでいる（ASSUMPTION）。fp32 の数値の差で字が変わりうるので、R2 が RP-18 の句読点の全件を CPU の provider でも掛け、`expected` と一致する件数を記録する。小さい model へ落ちた段の誤り率は測っていない（NOT VERIFIED）。
- **H42** Python の `final` は `t1_s` を持たないので、Python の run の遅れて着いた `final` は、前の窓が「final なし」で次の窓に `final` が 2 件以上在る形からの推定でしか数えられない（3.1）。推定の数が Python の p95 を小さく見せている量は分からない（NOT VERIFIED）。持ち主は R8h で、RP-32 の Python の run でその数を記録し、Rust の同じ数（`t1_s` で結んだ時に次の窓に着いていた `final` の数）と並べる。
- **H43** R8e の run の「訳」の段と `decode_ms` の差と、3.1 の数え方で数え直した `L`（p50 1533 ms、p95 2752 ms、母数 67。数え直す前より下がる）は、原因の席と試験の席の打ち直しの値で、設計の席は確かめていない（NOT VERIFIED）。数え直すと母数が 93 から 67 に減り、訳の取れた割合が RP-32 の 20 / 23 に届かない。`decode_ms` の差が負荷の差か engine の差かも分かっていない。Python の `decode_ms` が何を測った値か（待ちを含むか）と、発話の終わりの検出（`detected()`）の挙動も確かめていない。3.16 の 2 の A で、2 本並べた依頼が T の待ちを延ばす量も測っていない。持ち主は R8h で、RP-32 と RP-34 の run の段ごとの列で確かめ、2 の A を作るかの材料にする。
- **H90** 3.17 の発話 16 と 22 の中央値と、この H91、H92 の数のうち `83830af` の build の物は、RP-32 の 3 run の記録（git の外の `/home/fukugan/.cache/hayamimi-e2e-20261019/e-card-form/` の `rs1.jsonl`〜`rs3.jsonl`）から `jq` で数えた。git の中の `docs/results/rp32/` の `rs<N>` は前の build の run で、RP-73 の陽性対照はそちらの値で書いた。数え方は RP-67 と同じで、有効は `d = (t_final − t_end)/1000 − final_latency_ms ≥ 120`、`L = (t_tr − t_end)/1000`、nearest-rank である。この記録で数え方を確かめるため、起点を動かさない値が RP-32 の test の出力（`L` p50 885 ms、p95 1501 ms、n 291。Python 1427 ms と 1960 ms、n 285）と 1 ms 以内で一致することを見た。git の中へ写す持ち主は E2E の席である。
- **H91** 不確かの 11 発話（3.1）の声の本当の終わりは確かめていない（NOT VERIFIED）。下限は `end_sample`、上限は Silero の区切りの終わり（probe の threshold 0.5）と clip の終わりの小さい方と置いた。Silero が声の終わりより前で区切らないことは仮定である（ASSUMPTION。発話 18 は区切りが `end_sample` より 595 ms 前で終わり、上限には clip の終わりを当てた）。この幅で不確かの発話の起点を後ろへ動かした数（`83830af` の 3 run、線 0.12 秒）は次のとおりである。ja の Rust の `L` の p95 は、動かさない時 1,501 ms、高さ 25 % の終わりで 1,481 ms、15 % で 1,498 ms、上限で 1,509 ms、p50 は 885、748、734、666 ms である。Python の p95 は 1,959、1,950、1,903、1,807 ms である。後ろへ動かすほど p50 は下がるが、p95 は動かない。p95 を決める遅い発話（16、22、7）は確かと切れか、高さ 25 % で終わりが動かない発話だからである。認識器と別の model の token の時刻で声の終わりを取る強制位置合わせ（S38 の定義の形）は作らない。再開の条件は、RP-73 が `Err` を返した時、または全発話の `L` の p95 が 1000 ms の ±起点の幅に入った時である。
- **H92** RP-32 は Rust と Python が同じ `t_end` を使うので、起点を発話ごとに同じ量だけ動かしても対の比較の向きは変わらない。変わるのは終端不正の仕分けで、早い規則で確定した Rust の標本ほど線の下へ落ちる。`83830af` の 3 run で起点を動かした時の対の比較（Rust の線を 0.12 秒として数えた値）は、動かさない時 18/19、高さ 25 % で 18/19、15 % で 15/17（比較集合が 17 に減り、要るのは 15）、上限で 13/19（要るのは 17、落ちる）で、Rust の有効な標本は 294、294、260、215 である。起点を動かさずに組で絞ると、確かだけで 4/4（要るのは 4）、確かと切れで 9/10（要るのは 9）、不確かだけで 9/9（要るのは 8）で、遅い発話 22 は切れの組に在る。規則を変える時の Python の基準は、3.1 のとおり固定した記録（py1〜py3）を新しい `end_sample` で数え直し、Python は流さない（評議の判定）。`stream` の送りの時計は manifest の sample と線形で、`83830af` の rs1 の 23 発話で `(t_end_us − t_start_us)/1000 − (end_sample − start_sample)/16` が −20 ms（1 発話だけ −17 ms）の一定だった。規則を変えるかは評議が決める。
- **H93** 切れの発話で、Silero の区切りの終わりがつないだ無音の中へ 326〜665 ms 延びる理由は確かめていない（NOT VERIFIED）。sherpa-onnx の segment の終わりは、確率が `neg_threshold` 以下になった最初の window に当たる（S25）。window ごとの確率を記録していないので、延びが確率の尾か、clip の境の急な切れへの反応かは分からない。自然に終わる確かの ja の発話でも 100〜706 ms 延びる。持ち主は `L` の p95 の原因を調べる席で、probe に window ごとの確率を出す形は実装の席が足す。
- **H94** 3.1 の組の数と高さごとの声の終わりは、ffmpeg の `astats` で clip ごとに 320 sample の frame の RMS を取り、`awk` で `speech_end` と同じ式を当てた値である（作業の file は git の外の `/home/fukugan/.cache/hayamimi-rp67-20261020/`）。陽性対照として、同じ frame で 35 % の規則を当て直すと、38 発話の `end_sample` が 0 ms の差で再現した。ffmpeg の RMS と `frame_level_db`（i16 の二乗平均、−120 dB で床）が浮動小数の丸めまで同じかは確かめていない（ASSUMPTION）。RP-73 の Rust の関数が同じ組を出すかは、その test の陽性対照が確かめる。持ち主は RP-73 を実装する席である。
- **H95** 切れの 8 発話で、clip の終わりの音が声か声でない音かは確かめていない（NOT VERIFIED）。参照の文が clip の一部しか覆わない発話（12、13、14、19、21）が切れと不確かに入っている。声でなければ起点は声の終わりより後ろで、`L` は小さく出る（受入に甘い向き）。RP-73 は確かの発話だけで判定を照らすので、この向きで合否が割れればそこで落ちる。参照の文が短いことによる CER の偏りは、この穴の外である。
- **H96** 英語版 `rust_port.md` は 8 の R10 の成果物で、まだ無い（この文書の冒頭）。3.1、3.17、RP-67、RP-73、H90〜H96 の英語の文は R10 の席が日本語の正本から作る。

## 11. 出所

取得日は S19 と S20 が 2026-10-14、S21、S22、S25〜S37 が 2026-10-15、S38 と S39 が 2026-10-20、ほかはすべて 2026-10-13。S23 と S24 は別の席の調べ（2026-10-15）の転記で、この席は原典を開いていない。
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
| S21 | CAT-Translate の技術報告：3.3B の基の model は Sarashina 2.2 で、SFT の材料は社内の web の crawl、fineweb、Laboro-ParaCorpus、arXiv と PubMed と J-Stage の抄録、USPTO の特許と、それらから gpt-oss で作った対訳。validation は WMT 2021 の ja-en と WMT 2024 の文書単位の en-ja。FLEURS と FLoRes の名は無い | https://arxiv.org/html/2606.21413 |
| S23 | 言語モデルの訳の数の誤り：大きい単位の数の誤りが 8B の model でも最大で約 20% ある | 別の席の調べの転記。https://arxiv.org/abs/2501.04927 |
| S24 | 認識の前段に VAD を置くと、環境音での幻覚の率が 47.23% から 5.24% に下がり、本物の音声を落とす率は 0%（Whisper large-v3）。`no_speech_threshold` の調整だけでは 47% から 46% | 別の席の調べの転記。https://arxiv.org/abs/2609.04561 |
| S22 | TranslateGemma の技術報告：合成の対訳の原文は MADLAD-400、人の訳の対訳は SMOL と GATITOS、SFT に Gemma 3 の指示の data を 30% 混ぜる。本文を `pdftotext` で抜いて `fleurs` と `flores` は 0 行（同じ抜き出しで `wmt` は 12 行） | https://arxiv.org/pdf/2601.09012 |
| S25 | sherpa-onnx の Silero：声の始まりは確率が `threshold` を超えた時、終わりの無音は確率が `neg_threshold`（既定は `max(threshold − 0.15, 0.01)`）以下になった時から数える。segment の終わりは buffer の末尾から `min_silence` の sample を引いた所で、始まりは 2 window と最短の発話の分だけ前へ寄せる。`neg_threshold` は C++ の `silero-vad-model-config.h` に在り、C の API の `SherpaOnnxSileroVadModelConfig` には無い | https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/sherpa-onnx/csrc/silero-vad-model.cc 、https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/sherpa-onnx/csrc/voice-activity-detector.cc 、https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/sherpa-onnx/c-api/c-api.h |
| S26 | transducer の出力の遅れが終わりの判定を誤らせる。終わりの語の token と遅れの罰、frame ごとの声の判定の補助の網で直す（Switchboard で評価。abstract に数は無い） | https://arxiv.org/abs/2505.17070 |
| S27 | Phoenix-VAD：言語モデルで streaming に文の完結を判定する。実音声の文単位の正解率は完結 0.800、未完 0.838、同じ表の Smart Turn v3 は 0.704 と 0.623、TEN Turn Detection は 0.930 と 0.829。完結なら 400 ms、未完なら 1000 ms の無音で区切る（設定の値で、測った遅れではない）。判定 1 回の処理は 140 ms、判定の間隔は 320 ms | https://arxiv.org/abs/2509.20410 、https://arxiv.org/html/2509.20410 |
| S28 | 認識と終わりの判定を 1 つの多目的の model で持つと、別々の model より終わりの遅れが中央値で 120 ms（30.8%）、90 パーセンタイルで 170 ms（23.0%）縮み、voice search の WER は悪くならない（SLT 2022） | https://arxiv.org/abs/2211.00786 |
| S29 | 部分の認識結果で応答を先に取りに行き（prefetching）、RNN-T と LAS の 2 段の系で 200 ms 縮む。end-to-end の先読みの判定は、無音で決める形より同じ先読みの率で 100 ms 速い。先読みの回数だけ計算が増える（Interspeech 2020） | https://research.google/pubs/low-latency-speech-recognition-using-end-to-end-prefetching/ |
| S30 | 部分の発話から発話の全体を予測して応答を先読みする形。当たった予測の遅れの得と、外れた予測の計算の損の釣り合いを調べる（Interspeech 2023。abstract に数は無い） | https://arxiv.org/abs/2305.13794 |
| S31 | Smart Turn v3.2：音声だけを読む turn の終わりの判定の model。23 言語（ja を含む）、約 8M parameter（Whisper Tiny の骨組み）、CPU で 10 ms から、多くの cloud の機械で 100 ms 未満。Silero などの VAD が無音を見た後に、その turn の音声（最大 8 秒）に掛ける。BSD-2-Clause | https://github.com/pipecat-ai/smart-turn |
| S32 | ReazonSpeech の test の組（`japanese-asr/ja_asr.reazonspeech_test`）：test の split が 5,263 行、列は音声（0.62〜29.7 秒）と文字起こし（1〜169 字）、文字起こしに `。` と `、` が在る。頁に許諾の記載は無い | https://huggingface.co/datasets/japanese-asr/ja_asr.reazonspeech_test |
| S33 | llama-server の README：依頼の `cache_prompt` の既定は真で、真にすると結果が決まらないことがある。`--cache-reuse` の既定は 0。推測デコードの引数は `--spec-type`（既定 `none`）、`--spec-draft-model`（`-md`）、`--spec-draft-n-max`（既定 3）、`--spec-draft-n-min`（既定 0）、`--spec-draft-p-min`（既定 0.00）、`--spec-draft-sampling`（既定 `greedy`）、`--spec-default`。`--draft` と `--draft-min` は外され、`--spec-draft-n-max` か `--spec-ngram-mod-n-max` を使うと書かれている | https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md |
| S34 | llama.cpp の推測デコードの文書：下書きの model（`draft-simple`）、EAGLE-3、DFlash、DSpark、MTP と、model の要らない n-gram の型（`ngram-cache`、`ngram-simple`、`ngram-map-k`、`ngram-map-k4v`、`ngram-mod`）。`--spec-default` は `ngram-mod` を有効にする。n-gram の型は既に生成した文の型に頼む。出力を正確に合わせるには貪欲の sampling を使う | https://github.com/ggml-org/llama.cpp/blob/master/docs/speculative.md |
| S35 | 「Fast Inference from Transformers via Speculative Decoding」：近似の model が出した token を大きい model が並べて照らし、出力の分布を変えずに T5-XXL で 2〜3 倍速い（同じ出力）。既存の model を学習し直さずに使える | https://arxiv.org/abs/2211.17192 |
| S36 | `cyberagent/CAT-Translate-0.8b`：基の model は `sbintuitions/sarashina2.2-0.5b`、許諾は MIT。3.3b と tokenizer が同じかは書かれていない。同じ作者の CAT-Translate は 0.8b、1.4b、3.3b、3.3b-beta、7b の 5 つ | https://huggingface.co/cyberagent/CAT-Translate-0.8b 、https://huggingface.co/api/models?author=cyberagent&search=CAT-Translate |
| S37 | `mmnga-o/CAT-Translate-0.8b-gguf`：公開の GGUF の組。`CAT-Translate-0.8b-Q8_0.gguf` は 845,363,136 bytes、LFS の sha256 は `f652c0cd4352424a744bae9e9f633c2efd16a99c0f1a48a0e00d171dc9d6949f`。ほかに IQ3_M から Q6_K までの 13 の量子化が在る | https://huggingface.co/api/models/mmnga-o/CAT-Translate-0.8b-gguf/tree/main |
| S38 | 「Towards Fast and Accurate Streaming End-to-End ASR」（ICASSP 2020）：認識の遅れを、話し手が話し終えた時から最後の文字列が出るまでの時間と定め、終わりの判定の遅れを中央値（EP50）と 90 パーセンタイル（EP90）で測る。物差しの終わりは、通常の model で音声と書き起こしを強制位置合わせして取った、最後の無音でない音素の後の frame である | https://arxiv.org/abs/2004.11544 |
| S39 | Silero VAD の `get_speech_timestamps` と `VADIterator`：既定は `threshold` 0.5、`neg_threshold` は `max(threshold − 0.15, 0.01)`、`min_silence_duration_ms` 100、`speech_pad_ms` 30、`min_speech_duration_ms` 250（`VADIterator` には無い）。区切りの終わりは確率が `neg_threshold` を下回った最初の frame の始まりで、報告する終わりはそこへ `speech_pad_ms` を足した位置になる。sherpa-onnx の区切りは pad を足さない（S25） | https://github.com/snakers4/silero-vad の `src/silero_vad/utils_vad.py` |
