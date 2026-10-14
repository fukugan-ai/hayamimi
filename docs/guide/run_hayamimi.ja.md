# hayamimi（Rust 版の日本語の経路）を build して動かす

`native/hayamimi`（実行物）と `native/hayamimi-asr`（ライブラリ）は、日本語の音声を文字にして、字幕の event を `/events` で配ります。
音声は、発話の検出（VAD）、ReazonSpeech の認識、分割再試行、ITN、句読点、置換辞書の順に通ります。
GPU が無い機械でも止まりません。
GPU に置いた段（句読点）は CPU で動き、足りない物があればその段だけを外し、落ちたことを log、画面、stderr に 1 回ずつ出します。

```text
wav ─> VAD ─> 認識（int8、CPU）─> ITN ─> 句読点（GPU、無ければ CPU）─> 置換辞書 ─> final
                                                                              │
                                              log（stdout の JSONL）と /events（字幕）へ
```

図の結論：落ちる場面はどれも、終了 code 0 のまま、その段の event を出して続きます。

## build

GPU 版の sherpa-onnx の build（`build_gpu.ja.md`）が済んでいることが前提です。
`SHERPA_ONNX_LIB_DIR` は、その build の `install/lib` です。
Linux（WSL）では、ALSA の開発用の package も要ります（`sudo apt-get install -y libasound2-dev`、戻すときは `sudo apt-get remove libasound2-dev`）。取り込みの `cpal` が `alsa-sys` を引き、これが無いと `hayamimi` と `hayamimi-eval` の build が `alsa-sys` の build script で止まるからです。

```bash
# WSL
SHERPA_ONNX_LIB_DIR=<repo>/native/target/sherpa-gpu/linux/install/lib cargo build --release -p hayamimi
```

```powershell
# Windows
$env:SHERPA_ONNX_LIB_DIR = "<repo>\native\target\sherpa-gpu\windows\install\lib"
cargo build --release --target x86_64-pc-windows-msvc -p hayamimi
```

並列度は `~/.cargo/config.toml` の 2 のままにします。
`-j` は付けません。

ORT と sherpa-onnx の共有ライブラリは、実行物の隣に置かれます。
Windows の test の実行物は `target/<profile>/deps/` に出るので、1 つ上の directory からライブラリを先読みします（`hayamimi_gpu::runtime::preload_near_executable`）。

## model を取得する

認識、言語判定、VAD、句読点の model は、`hayamimi models fetch` が取得します。
Python も別の script も要りません。

```bash
hayamimi models fetch --minimal --dir models   # 日本語と英語の核の 4 組だけ
hayamimi models fetch --dir models             # 他の言語の認識器と話者の model も
```

| option | 意味 |
|---|---|
| `--minimal` | 認識（ReazonSpeech の int8）、言語判定（whisper-tiny）、VAD（Silero）、句読点（BERT）の 4 組 |
| `--en-parakeet-v2` | 英語専用の Parakeet v2 の認識器も取得する（`--minimal` と併せられる） |
| `--punct-4class` | 4 クラスの日本語句読点 model も取得する（`--minimal` と併せられる） |
| `--dir` | model を置く directory。既定は `models` |

組ごとに、sherpa-onnx の release から先に、Hugging Face から後に取得します。
1 組の file が全部揃うまで、`models/` には何も置きません（隣の一時 directory に書き、揃ってから 1 file ずつ rename します）。
揃っている組は取得し直しません。
`--dir` の下の `manifest.json` に、file ごとの段、sha256、bytes を書きます。内容が変わらなければ書き直しません。

どの取得先からも得られない組があっても、既に在る file はそのまま残し、`models.fallback.previous` を 1 件出して、終了 code は 0 のままです。
その組の段は、`hayamimi` を動かすときに外れます（`model.fallback.disabled`）。
取得先の address を替える `--release-url` と `--hf-url` は、取得先を壊した test のために在ります。

Silero の VAD は、release の file と Hugging Face の file の bytes が違います。2 つが同じ model かどうかは確かめていません（NOT VERIFIED）。

## 動かす

```bash
hayamimi --wav lat-ja.wav --models <models の directory>
```

| option | 意味 |
|---|---|
| `--wav` | 16 bit の wav。既定は音声の速さで流す。`--no-realtime` で続けて流す |
| `--serve [port]` | `/events` を 127.0.0.1 で配る。port の既定は 8833 |
| `--min-silence` | 発話の終わりにする無音の秒数。既定は 0.30 |
| `--replace` | 置換辞書。読めなければ使わずに続ける |
| `--gpu-diagnose` | 句読点の model の node の置き場を log に出す |

log は JSONL で stdout に出ます。
字幕の文字と file の path は入りません。
終了 code が 0 以外になるのは、入力の wav が読めないなど、代わりの道が無い失敗だけです。

## 画面（Dioxus の 3 頁）を配る

`--serve` の server は、`/`、`/dashboard`、`/transcript` に `native/hayamimi-ui/web/index.html` を返します（実行物に組み込み済み）。
`/ui/*` は、実行物の隣の `ui/` directory の file（wasm-bindgen の出力）を返します。
`ui/` に無い名前、directory、`..` で外へ出る名前は 404 です。
`ui/` が無いと、頁の殻は返りますが wasm と js が 404 になり、画面は空のままです。

`ui/` は次の 2 つの command で作ります。
`wasm32-unknown-unknown` の target と、`wasm-bindgen` の CLI（crate の `wasm-bindgen` と同じ版）が要ります。

```bash
cd native
cargo build --release --target wasm32-unknown-unknown -p hayamimi-ui --features web
wasm-bindgen --target web --no-typescript --out-dir target/release/ui --out-name hayamimi-ui \
    target/wasm32-unknown-unknown/release/hayamimi-ui.wasm
```

`target/release/ui` は `target/release/hayamimi` の隣です。
持ち出すときは、実行物と `ui/` を同じ directory に置きます。
頁は同じ origin の `/events` を読みます。

受入は `cargo test --release -p hayamimi --test r6` です。
この test は上の 2 つを自分で打ち、本物の `hayamimi --wav ja_014.wav --serve` を起こします。
確かめるのは、3 頁が `web/index.html` と同じ bytes であること、index とその script が読む `/ui/` の file が全部 200 で bytes も同じであること、無い名前と `..` が 404 であることです。

**採らない手段**：

- wasm を実行物に組み込む。`hayamimi` の build が wasm の build を待つことになるので採りません。
- 配る directory を option や環境変数で変える。置き場が 2 つ以上になって指し間違えるので採りません。
- 旧来の `data/pages/*.html` を、`ui/` が無いときの代わりに返す。どの route からも返さない形にしました。file は残してあります。

## 落ちるときの形

起動の時に、GPU を 2 段で確かめます。
1 段目は CUDA の device の数です。
0 個か driver が読めなければ `gpu.device.unavailable` を出し、`reason` は `no_device` か `driver_unreadable` になります。
2 段目は句読点の CUDA の session です。
device が在って session が作れなければ、その段だけを CPU で作り直します（`session_failed`）。

| 場面 | log の event（warn） | `/events` の `fallback`（`stage`、`to`） |
|---|---|---|
| 句読点が CPU へ | `gpu.fallback.cpu` | `punct`、`cpu` |
| model の file が無いか読めない（VAD、認識器、句読点） | `model.fallback.disabled` | `vad`、`recognizer`、`punct`。`to` は `disabled` |
| 推論の library（sherpa-onnx、ORT）が開けない | `asr.fallback.disabled` | `asr`、`disabled` |
| port が使われている | `server.fallback.port`（`url` に実際の URL） | `events`、`next_port`（10 個とも塞がっていれば `disabled`） |
| 置換辞書が読めない | `config.fallback.default` | `replace`、`default` |

次の番号から 10 個までの port を試します。
遅れて接続した client や接続し直した client には、効いている `fallback` と `session_start` が、その順で最初にまとめて届きます。
stderr には、落ちた段、理由、落ち先の 3 行を locale の言葉で出します。
GPU が原因なら、`nvidia-smi` で確かめる案内の 1 行を足します。

推論の library が開けない場面は、Windows の実行物でだけ起こせます。
Linux の実行物は、共有ライブラリを起動前に loader が解決します。
library を外すと、`main` に入る前に止まります。

## 遅れ `l_dec_ms` を測る

`asr.utterance.timed` の `l_dec_ms` は、VAD が区切りを渡してから、ITN、句読点、置換辞書を終えた文字ができるまでの時間です。
p50 と p95 は nearest-rank で取ります（`hayamimi_eval::percentile::nearest_rank` と同じ式）。

```bash
jq -s -r '[.[]|select(.event=="asr.utterance.timed")|.l_dec_ms]|sort as $v|($v|length) as $n|"n=\($n) p50=\($v[(($n*0.5)|ceil)-1]) p95=\($v[(($n*0.95)|ceil)-1])"' docs/results/r2/wsl/lat_ja_l_dec_*.jsonl
```

結果は `docs/results/r2/l_dec_ms.md` に置きました。

## ITN と句読点の後処理の CPU の時間

```bash
hayamimi-eval stages --provider cpu  --wav lat-ja.wav --models <models> --only itn,punct_rules --repeat 5 --run 1 --out docs/results/r2/win/string_stages_cpu_idle_1.jsonl
hayamimi-eval stages --provider cuda --wav lat-ja.wav --models <models> --only itn,punct_rules --repeat 5 --run 1 --out docs/results/r2/win/string_stages_cuda_idle_1.jsonl
```

`cuda` の run は、この 2 段に GPU の置き場を作ったときの下限（転送の往復だけの時間）を作ります。
`--run` を 1、2、3 と重ねたら、`stages-report` で `gpu_stages.md` を作り直します。
`gpu_stages.md` は手で書き換えません。

```bash
hayamimi-eval stages-report \
  --runs docs/results/r1/win/final_*.jsonl docs/results/r1/win/r1b_*.jsonl docs/results/r2/win/string_stages_c*_idle_*.jsonl docs/results/r4/win/cluster_c*_idle_*.jsonl \
  --design docs/design/rust_port.ja.md --build-json native/target/sherpa-gpu/windows/build.json --cpu "<cpu の名前>" \
  --transfers docs/results/r1/transfers.json --placed docs/results/r1/placed/win_*_cuda.json \
  --python testdata/baseline/lat_ja_0.35_1.jsonl testdata/baseline/lat_ja_0.35_2.jsonl --out docs/results/gpu_stages.md
```

`docs/results/r1/transfers.json` は、`gpu_stages.md` の「転送」の表の 6 行を、そのまま JSON に戻した物です。
上の command は、R1 の 10 本と R1b の 10 本の run file から、R2 の行を足す前の `gpu_stages.md` を 1 文字も違わずに作り直せることを確かめてあります。

## 清書の clustering の CPU の時間

FastClustering は sherpa-onnx の中で再分離と同じ呼び出しに含まれ、単独で呼ぶ口がありません。
そこで同じ算法（cosine 距離、完全連結、閾値 0.5）の Rust の実装 `hayamimi_asr::cluster` を測ります。
`cluster` の段が測るのは、発話 12 個の埋め込みの clustering、cluster ごとの平均、重心 8 個への全体の対応づけまでです。
埋め込みはこの段が自分で作り（時間には入れません）、`--only cluster` だけで打てます。

```bash
hayamimi-eval stages --provider cpu  --wav lat-ja.wav --models <models> --only cluster --repeat 5 --run 1 --out docs/results/r4/win/cluster_cpu_idle_1.jsonl
hayamimi-eval stages --provider cuda --wav lat-ja.wav --models <models> --only cluster --repeat 5 --run 1 --out docs/results/r4/win/cluster_cuda_idle_1.jsonl
```

`--run` を 1、2、3 と重ねたら、上の `stages-report` の `--runs` に `docs/results/r4/win/cluster_c*_idle_*.jsonl` を足して、`gpu_stages.md` を作り直します。

## 話者の分離の DER

`der` は、評価の set（`manifest.json`、wav、RTTM）の会議ごとに DER を採点します。
流れは、VAD、発話ごとの速報の話者、2 秒の無音か 25 秒で区切る group、group ごとの再分離と全体の対応づけ、simpleder と同じ式の採点です。
認識は掛けません。

出力の 1 行の形は Python の `eval_diar.py --method refine_diarize` と同じで、`--out` の file にも同じ文字が入ります。
`--min-silence` の既定は Python の run と同じ 0.35 です。
`--models` を付けなければ、cwd から上へ向かって `models/silero_vad.onnx` を持つ `models/` を探します。

```bash
hayamimi-eval der --set testdata/eval_diar --collar 0.25 --out docs/results/r4/der_rust_run1.log
cargo test --release -p hayamimi-eval --test rp15 -- --ignored
```

Rust の run は `docs/results/r4/der_rust_run<n>.log` に置きます。
test は、Rust の run の平均と、`testdata/baseline/` の Python の run（3 回以上）の平均と幅を比べ、結果を `docs/results/r4/rp15_der.md` に書きます。

## 話者の印を wire で受ける

`final` の `speaker` は常に空です。
埋め込みが済むと、同じ `id` の `{"type":"speaker","id":"file-0","speaker":"S1"}` が `final` の後に 1 回だけ届きます。
`S1?` のように `?` が付く印は、まだ 1 回しか一致していない話者です。
話者の model が無いか、埋め込みに失敗した発話には `speaker` の event が出ません。

字幕の遅れに印が入らないことは、話者の model を外した directory との比較で確かめます。
差は `asr.utterance.timed` の `l_dec_ms` の p50（話者あり − 話者なし）で、上限は 5 ms です。

```bash
cargo test --release -p hayamimi --test rp15_speaker -- --include-ignored
```

## worktree で test を打つ

worktree には、GPU 版の sherpa-onnx の build（`native/target/sherpa-gpu`）がありません。
`SHERPA_ONNX_LIB_DIR` が空のままだと、sherpa-onnx の crate が CPU 版を取りに行き、`libonnxruntime.so.1` が見つからず 15 本の test が `dlopen failed` で落ちます。

cargo は作業 directory から上へ向かって `.cargo/config.toml` を全部読みます。
worktree は本体の checkout の `.claude/worktrees/` の下にあるので、本体の直下に次の 1 file を置くと、本体も全部の worktree も同じ build に繋がります。
この file は git の管理に入れず（`.git/info/exclude` に `/.cargo/` を書く）、機械ごとに置きます。

```toml
[env]
SHERPA_ONNX_LIB_DIR = { value = "native/target/sherpa-gpu/linux/install/lib", relative = true }
```

`relative = true` の起点は、この file を置いた directory（本体の直下）です。
環境変数が先に設定されていれば、そちらが勝ちます。
Windows の cargo も同じ file を読むので、Windows では `SHERPA_ONNX_LIB_DIR` に `native\target\sherpa-gpu\windows\install\lib` を毎回設定します。

置いたあとは、worktree の `native` で追加の指定なしに打てます。

```bash
cargo test --release -p hayamimi -p hayamimi-asr -- --include-ignored
```

## test

| 何を | 打ち方 |
|---|---|
| 既定の test（WSL） | `SHERPA_ONNX_LIB_DIR=<install/lib> cargo test --release`（上の file があれば指定は要らない） |
| 実走の test | `cargo test --release -p hayamimi --test rp06 --test rp16 --test rp26 --test rp29 -- --ignored` |
| RP-21（`models fetch`。実際に取得するので network を使う） | `cargo test --release -p hayamimi --test rp21 -- --include-ignored` |
| 文書の link（RP-19） | `cargo run --release -p hayamimi-eval -- doc-links --repo <repo>` |
| RP-2、RP-18（GPU を使う） | `cargo test --release -p hayamimi-asr --test rp02 --test rp18 -- --include-ignored` |

GPU を隠す環境変数は、Linux では `CUDA_VISIBLE_DEVICES` を空にします。
Windows の driver は空の値を無視するので、`-1` にします。
Windows は言語を system の設定から取り、`LANG` では変わりません。
言語の比較（en と ja）は Linux でだけ打ちます。

## RP-3 の run を録る（R8e）

認識、中継、訳の server を起こし、`hayamimi-eval stream` で `lat-ja.wav` を 5 回流します。
訳の server は user unit `llama-artisan@CAT-Translate-3.3b-Q8_0.service`（port 8080、slot 4）です。

```bash
systemctl --user start 'llama-artisan@CAT-Translate-3.3b-Q8_0.service'
hayamimi --input ws --ws-port 8766 --serve 8833 --models <models> --min-silence 0.30
hayamimi-relay live --events http://127.0.0.1:8833/events --llm http://127.0.0.1:8080 --serve 8835
hayamimi-eval stream --wav testdata/lat-ja.wav --repeat 5 --port 8766 --relay-port 8835 --out docs/results/r8e/rp3_rust.jsonl
```

終えたら `hayamimi` と `hayamimi-relay` を起こした時の pid で止め、`systemctl --user stop` で server を止めます。
`rp3_rust.jsonl` と `rp3_rust.events.jsonl` を、RP-5（`cargo test --release -p hayamimi-eval --test rp05 -- --ignored`）と RP-25（`--test rp25 -- --ignored`）が読みます。
GPU の memory は、同じ間に `nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits` を 1 秒ごとに読んで RP-22 に使います。
H6 と `cache_prompt` は `cargo test --release -p hayamimi-relay --test r8e_server -- --ignored` です（server を起こしたまま打ちます）。
結果は `docs/results/r8e/README.md` です。

## WSL の llama.cpp を戻す

WSL の llama.cpp を b11577 から b11628 へ上げた時、前の build を tar に控えました。
`<backup>` は、作業 directory と別の木に置いた backup の directory です。
`<llama.cpp>` は WSL の llama.cpp の checkout です。

| 物 | 置き場 |
|---|---|
| 前の build | `<backup>/llama-cpp-b11577-build-cuda120-20261014.tar`（tar の中は `build-cuda120/`。symlink を含むので、symlink を作れない file system には展開しません） |
| 前の checkout の commit | `<backup>/llama-cpp-b11577-build-cuda120-20261014.HEAD`（`ec483d3f3`） |

戻す前に、translation server の unit を止めます。

```bash
systemctl --user stop 'llama-artisan@CAT-Translate-3.3b-Q8_0.service'
cd <llama.cpp>
mv build-cuda120 build-cuda120.b11628
tar -xf <backup>/llama-cpp-b11577-build-cuda120-20261014.tar
git checkout "$(cat <backup>/llama-cpp-b11577-build-cuda120-20261014.HEAD)"
```

確かめます。`/props` の `build_info` が `b11577` で始まれば戻っています。

```bash
systemctl --user start 'llama-artisan@CAT-Translate-3.3b-Q8_0.service'
curl -s http://127.0.0.1:8080/props | jq -r .build_info
```

戻したら、`build-cuda120.b11628` は不要になった時に消します。
上げ直すには `git pull --rebase` の後に `cmake --build build-cuda120 --config Release` を打ちます。

## 戻し方

`native/Cargo.toml` の `members` から `hayamimi` と `hayamimi-asr` を外し、2 つの directory を消すと、この slice の前の形に戻ります。
`hayamimi-gpu` の追加（`preload_near_executable`、`require_near_executable`）と、`hayamimi-eval` の `itn`、`punct_rules` の段は、commit を revert します。

## 未検証

- `rp06` は GPU 全体の `memory.used` で受けます。起動前 3 回の中央値を基準に、実行中の最大が 256 MiB 以上増え、終了後 10 秒以内に基準の ±64 MiB へ戻れば通ります。256 と 64 は仮の値です。
  `--query-compute-apps` は、WSL では process 名が `[Not Found]`、Windows の WDDM では使用量が `[N/A]` になるので使いません。
- 推論の library を外した run（`missing_library_in_a_real_run_keeps_the_server`）は Windows の実行物でだけ打てます。
- 負荷の下の ITN と句読点の後処理の時間は測っていません。`gpu_stages.md` の「負荷」の行は未測定です。
