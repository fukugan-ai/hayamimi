# GPU 版 sherpa-onnx の build と接続

Rust 版の認識を NVIDIA の GPU（CUDA）で動かすために、sherpa-onnx の C ライブラリを自分で build し、Rust の crate をそこへ繋ぐ手順です。
Windows で 1 本の音声を GPU の上で認識するところまでを、実際に打って確かめてあります。
WSL（Linux）での build は、まだ作っていません。

## 結果

`cargo run -p xtask -- sherpa-gpu` が、sherpa-onnx v1.13.8 を CUDA 13.4 と ONNX Runtime 1.28.2 の CUDA 13 版で build し、動かすのに要る DLL を 1 つの directory に集めます。
`hayamimi-gpu-probe` がその directory の DLL を使って 1 本の音声（9.6 秒）を開き、認識が終わるまで GPU 用の provider（ONNX Runtime が計算を GPU へ回す部品）が読み込まれていたことを JSON 1 行で報告します。

```text
build の流れ
  git clone (v1.13.8) ─┐
  ORT 1.28.2 の zip ───┼─> cmake configure/build/install ─> install/lib に DLL を集める ─> build.json
  CUDA 13.4 / cuDNN 9 ─┘                                                   │
                                                                           v
                              SHERPA_ONNX_LIB_DIR=install/lib ─> cargo build ─> hayamimi-gpu-probe
```

図の結論：build は 1 本の xtask で終わり、Rust 側は `SHERPA_ONNX_LIB_DIR` に `install/lib` を渡すだけで繋がります。

## 前提

| 物 | 版 | 確認に使った場所 |
|---|---|---|
| OS | Windows 11 x64 | |
| GPU | NVIDIA GeForce RTX 5060 Ti（compute capability 12.0、driver 610.88） | `build.json` の `gpu_*` |
| CUDA Toolkit | 13.4（nvcc 13.4.59） | `build.json` の `cuda_version` |
| cuDNN | 9.27.0 の CUDA 13 用 archive | `build.json` の `cudnn_version` |
| ONNX Runtime | 1.28.2 の `gpu_cuda13` | `build.json` の `ort_package` |
| sherpa-onnx | v1.13.8（commit `11afbd009a7f8c08f4bcf2fc1b265d0df4670fbf`） | `build.json` の `sherpa_onnx_commit` |
| Rust | 1.99.0（msvc） | |

道具として、Visual Studio の Build Tools（`dumpbin` と Ninja を含む Developer PowerShell）、`cmake`、`git`、`curl`、`tar` が PATH に要ります。
Windows には CUDA 11.2 と 12.3 も入っていることがあり、PATH の先頭にあるのはその古い `nvcc` です。
そのため xtask は PATH の `nvcc` を使わず、`--cuda-dir` で渡した directory の `bin\nvcc.exe` を使います。

## 手順

### 1. build する

Visual Studio の Developer PowerShell を開き、repo の `native` directory で打ちます。
`<...>` は自分の環境の directory に置き換えます。

```powershell
Enter-VsDevShell -VsInstallPath "<Visual Studio の install 先>" -DevCmdArguments "-arch=x64"
cd <repo>\native
cargo run --release -p xtask -- sherpa-gpu --cuda-dir "<CUDA 13.4 の directory>" --cudnn-dir "<cuDNN 9 の CUDA 13 用 archive を展開した directory>"
```

既定の置き場は `native\target\sherpa-gpu` で、git の管理外です。
別の場所に作るときは `--out-dir` を付けます。
版を変えるときは `--sherpa-tag` と `--ort-version` を付けます。
すでに展開した ORT があるときは `--ort-dir` で渡すと、取得を省きます。

xtask は次の順に 1 段階ずつ進み、失敗したらその段階の名前を言って止まります。

1. sherpa-onnx の指定 tag を浅く clone する。
2. ORT の zip を取り、展開する。
3. cmake で configure、build（並列度 2）、install する。
4. CUDA と cuDNN の DLL と `onnxruntime.lib` を `install\lib` へ集める。
5. 版、DLL の SHA-256、provider の対応 GPU 世代を `build.json` へ書く（一時 file に書いて rename するので、途中で止まっても半端な `build.json` は残らない）。

### 2. Rust から繋ぐ

`SHERPA_ONNX_LIB_DIR` に `install\lib` を渡して build します。
`sherpa-onnx-sys` の build script がこの値を読み、`sherpa-onnx-c-api` と `onnxruntime` に link し、その directory の `*.dll` を `target\<profile>` へ複写します。

```powershell
cd <repo>\native
$env:SHERPA_ONNX_LIB_DIR = "<repo>\native\target\sherpa-gpu\windows\install\lib"
cargo build --release --target x86_64-pc-windows-msvc -p hayamimi-gpu
```

`--target` を付けるので、実行物と DLL は `target\x86_64-pc-windows-msvc\release` に並びます。

### 3. 1 本の音声を GPU で開く

```powershell
$release = "<repo>\native\target\x86_64-pc-windows-msvc\release"
$model = "<モデルの directory>\sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17"
& "$release\hayamimi-gpu-probe.exe" --encoder "$model\encoder-epoch-35-avg-1.onnx" --decoder "$model\decoder-epoch-35-avg-1.onnx" --joiner "$model\joiner-epoch-35-avg-1.onnx" --tokens "$model\tokens.txt" --wav "<repo>\tests\golden\ja\ja_014.wav"
```

出力は JSON 1 行で、読む欄は次の 3 つです。
`loaded_provider` が `cuda`、`cuda_devices` が 1 以上、`onnxruntime_libraries` が 1 であれば、GPU で開けています。
`modules` の各行は、読み込まれた DLL の名前と、それが実行物と同じ directory から来たか（`exe_directory`）、`System32` から来たか（`system`）、それ以外か（`elsewhere`）を示します。

`hayamimi-gpu-probe --modules` を単独で打つと、モデルを開かずに、その時点で読み込まれている DLL だけを報告します。

## 実測した結果

RTX 5060 Ti で `ja_014.wav`（9600 ms）を 3 回続けて開いた結果です（同じ音声、同じ build、cache の状態は揃えていません）。

| 欄 | 値 |
|---|---|
| `loaded_provider` | `cuda`（3 回とも） |
| `cuda_devices` | 1 |
| `ort_version` | `1.28.2` |
| `onnxruntime_libraries` | 1 |
| `decode_ms` | 660、707、658 |
| `decoded_chars` | 44 |

この 3 回より前に、`decode_ms` が 2490 の実行が 1 回ありました。
その実行は module の snapshot を認識器の解放後に取っていたため `loaded_provider` が `cpu` と出ましたが、その表示は誤りで、実際にどの provider で動いたかは分かりません。
2490 ms が初回の初期化を含むのかも切り分けていません。
GPU と CPU の速さの比較は、音声の長さ、warm-up、反復を揃えた別の測定で行います。

認識が終わるまで読み込まれていた DLL のうち、CUDA と ONNX Runtime に関わる物は次のとおりです。

- 実行物と同じ directory：`onnxruntime`、`onnxruntime_providers_cuda`、`onnxruntime_providers_shared`、`sherpa-onnx-c-api`、`cublas64_13`、`cublaslt64_13`、`cudnn64_9`、`cudnn_graph64_9`、`cudnn_ops64_9`、`cudnn_heuristic64_9`、`cudnn_engines_precompiled64_9`、`cudnn_engines_runtime_compiled64_9`、`cudnn_engines_tensor_ir64_9`
- NVIDIA の driver 側：`nvcuda`、`nvcuda64`、`nvcudart_hybrid64`、`nvapi64`、`nvapi64_impl`、`nvdxgdmal64`、`nvobjectloader64`

`cudart64_12.dll` も `cudart64_13.dll` も読み込まれていません。
`install\lib` には CUDA の `cudart64_13.dll` を集めてありますが、使われていません。
`onnxruntime_providers_cuda.dll` の依存を `dumpbin /dependents` で読むと、`cublasLt64_13.dll`、`cublas64_13.dll`、`onnxruntime_providers_shared.dll`、KERNEL32 と CRT だけで、`cudart` は並びません。
CUDA runtime はこの provider に静的に含まれていると読めますが、含まれていると確かめる検査は打っていません。
CUDA 13 の裏付けは、`cublas64_13.dll`、`cublasLt64_13.dll`、cuDNN 9 の DLL が読み込まれていることです。

provider 内の GPU 向け機械語は、`cuobjdump --list-elf` で sm_75、sm_80、sm_86、sm_89、sm_90a、sm_100a、sm_120a が出ます。
`sm_120` の機械語は無く、sm_120 は PTX（実行時に GPU 向けへ変換される中間表現）として 235 件あります。
RTX 5060 Ti（compute capability 12.0）は、この PTX から実行時に変換されて動いていると読めます。
変換に掛かる初回の遅延は測っていません。

## 標準の手順から変えた点

### cmake の option

設計の表（`docs/design/rust_port.ja.md` の 3.12）の option は `-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON -DSHERPA_ONNX_ENABLE_GPU=ON` と、TTS を外す option です。
xtask は次の 4 つを足して無効にしています。

| 足した option | 無効にした物 |
|---|---|
| `-DSHERPA_ONNX_ENABLE_PORTAUDIO=OFF` | 音声の入出力 library |
| `-DSHERPA_ONNX_ENABLE_WEBSOCKET=OFF` | WebSocket の server と client |
| `-DSHERPA_ONNX_ENABLE_BINARY=OFF` | 付属の command line 実行物 |
| `-DSHERPA_ONNX_BUILD_C_API_EXAMPLES=OFF` | C API の example |

hayamimi-gpu が使うのは C ライブラリ（`sherpa-onnx-c-api`）だけなので、取得物と build の量を減らすために外しました。
この 4 つを外さなかったときに失敗するか、時間がどれだけ違うかは測っていません。
生成器は Ninja（`-G Ninja`）です。

### 環境変数は `/` 区切りで渡す

sherpa-onnx の cmake は、ORT の場所を環境変数 `SHERPA_ONNXRUNTIME_INCLUDE_DIR` と `SHERPA_ONNXRUNTIME_LIB_DIR` から読みます。
この 2 つに `\` 区切りの path を渡すと、`cmake --install` が `Invalid character escape '\U'` で止まります。
cmake が値をそのまま script の文字列に埋め込み、`\U` を escape と読むためと考えられます。
xtask は path の `\` を `/` に直してから渡します。
手で cmake を打つときも、この 2 つは `C:/...` の形で渡します。

この 2 つの値は cmake が読む値で、hayamimi の Rust のコードは環境変数を読みません。
Rust 側で環境変数を読むのは、`sherpa-onnx-sys` の build script が読む `SHERPA_ONNX_LIB_DIR` だけです。

## Windows に同梱の onnxruntime.dll との衝突

Windows には、Windows ML 用の `C:\Windows\System32\onnxruntime.dll`（1.17.1）が入っています。
`sherpa-onnx-c-api.dll` が名前だけで `onnxruntime.dll` を import すると、実行物の隣の 1.28.2 ではなくこの 1.17.1 に結びつく場合があります。
そのとき、起動は次の文言で異常終了します（終了コード -1073741819）。

```text
The requested API version [28] is not available ... Current ORT Version is: 1.17.1
```

実行ごとに結びつく先が変わる場合があり、どの条件で System32 側に結びつくかは突き止めていません。
`.local` file による探索順の変更も試しましたが、効きませんでした。

hayamimi-gpu は次の 2 つで防いでいます。

1. 実行物を link するときに `sherpa-onnx-c-api.dll` を遅延読み込みにする（`native/hayamimi-gpu/build.rs` が `/DELAYLOAD:sherpa-onnx-c-api.dll` と `delayimp.lib` を渡す）。
2. 最初の sherpa-onnx の呼び出しの前に、実行物の directory の `onnxruntime.dll` を完全な path で読み込む（`runtime::preload_beside_executable`）。

先に実行物の隣の DLL が読み込まれていれば、後から来る `onnxruntime.dll` の import はその読み込み済みの物で解決されます。

`hayamimi-gpu` を link する実行物（test を含む）は、すべて同じ 2 つの link 引数が要ります。
library の crate は、依存する側の link 引数を決められないためです。
`build.rs` が渡すのは `bins` だけで、test の実行物はまだ作っていないため含めていません。

## 採らない手段

採ったのは、利用者が指定した「sherpa-onnx の C ライブラリを CUDA 13.4 と ORT 1.28.2 の `gpu_cuda13` で自分で build し、`SHERPA_ONNX_LIB_DIR` で繋ぐ」形です。
次の手段は、理由とともに採りません。

- **CUDA 12 版の build を主にすること**：sherpa-onnx の既定は CUDA 12 版の ORT ですが、CUDA 13.4 の環境があるので使いません。CUDA 13 版が動かないときの退避先としてだけ残してあり、手順は設計の 3.12 にあります。
- **公開済みの CUDA 版を取ること**：`sherpa-onnx-sys` の build script は CUDA 版を自分では取りません。版と option を固定して再現するために、自分で build します。
- **cloud の認識 API と実行時の Python**：速度を最優先にするため使いません。
- **`ort` crate を直接使うこと**：既定の api が 1.31 以上で、固定した 1.28.2 を受け付けません。1.31.0 への移行か api-28 の指定かは設計の側で決めるため、`hayamimi-gpu` は `ort` に依存していません。
- **`native/hayamimi-eval` の変更**：この build と接続は触りません。

## 戻し方

作った物はすべて `native\target\sherpa-gpu` の下にあり、git の管理外です。
消すと build 前に戻り、他の物は変わりません。

```powershell
Remove-Item -Recurse -Force "<repo>\native\target\sherpa-gpu"
```

Rust 側を CPU 版に戻すには、`SHERPA_ONNX_LIB_DIR` を設定せずに build します。
build script が CPU 版の archive を取ります（設計 3.12）。
CUDA 12 版へ戻す手順は、設計の 3.12 に書いてあります。

## 未検証

- WSL（Linux）での build。xtask は Windows 以外では `Error::UnsupportedOs` で止まります。WSL に cuDNN 9（CUDA 13 版）は入っておらず、入れる手段と消し方は、評議の GO を得る前にこの文書へ書きます。今は未記入です。
- 認識結果の文字数が 44 で、golden の期待値（45 文字）と 1 文字違います。文字そのものは比べていません。
- System32 の DLL に結びつく条件。上の遅延読み込みと事前読み込みは、3 回とも防げたことだけを確かめています。
- `sm_120` を PTX から変換する初回の遅延。
- ONNX Runtime の新しい版（1.31.0、1.28.3）への移行。
