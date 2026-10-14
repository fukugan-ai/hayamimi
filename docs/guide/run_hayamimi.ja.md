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
  --runs docs/results/r1/win/final_*.jsonl docs/results/r1/win/r1b_*.jsonl docs/results/r2/win/string_stages_c*_idle_*.jsonl \
  --design docs/design/rust_port.ja.md --build-json native/target/sherpa-gpu/windows/build.json --cpu "<cpu の名前>" \
  --transfers docs/results/r1/transfers.json --placed docs/results/r1/placed/win_*_cuda.json \
  --python testdata/baseline/lat_ja_0.35_1.jsonl testdata/baseline/lat_ja_0.35_2.jsonl --out docs/results/gpu_stages.md
```

`docs/results/r1/transfers.json` は、`gpu_stages.md` の「転送」の表の 6 行を、そのまま JSON に戻した物です。
上の command は、R1 の 10 本と R1b の 10 本の run file から、R2 の行を足す前の `gpu_stages.md` を 1 文字も違わずに作り直せることを確かめてあります。

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
| RP-2、RP-18（GPU を使う） | `cargo test --release -p hayamimi-asr --test rp02 --test rp18 -- --include-ignored` |

GPU を隠す環境変数は、Linux では `CUDA_VISIBLE_DEVICES` を空にします。
Windows の driver は空の値を無視するので、`-1` にします。
Windows は言語を system の設定から取り、`LANG` では変わりません。
言語の比較（en と ja）は Linux でだけ打ちます。

## 戻し方

`native/Cargo.toml` の `members` から `hayamimi` と `hayamimi-asr` を外し、2 つの directory を消すと、この slice の前の形に戻ります。
`hayamimi-gpu` の追加（`preload_near_executable`、`require_near_executable`）と、`hayamimi-eval` の `itn`、`punct_rules` の段は、commit を revert します。

## 未検証

- `rp06` は GPU 全体の `memory.used` で受けます。起動前 3 回の中央値を基準に、実行中の最大が 256 MiB 以上増え、終了後 10 秒以内に基準の ±64 MiB へ戻れば通ります。256 と 64 は仮の値です。
  `--query-compute-apps` は、WSL では process 名が `[Not Found]`、Windows の WDDM では使用量が `[N/A]` になるので使いません。
- 推論の library を外した run（`missing_library_in_a_real_run_keeps_the_server`）は Windows の実行物でだけ打てます。
- 負荷の下の ITN と句読点の後処理の時間は測っていません。`gpu_stages.md` の「負荷」の行は未測定です。
