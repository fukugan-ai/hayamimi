# GPU 版 sherpa-onnx の build と接続

Rust 版の認識を NVIDIA の GPU（CUDA）で動かすために、sherpa-onnx の C ライブラリを自分で build し、Rust の crate をそこへ繋ぐ手順です。
Windows と WSL（Linux）の両方で、1 本の音声を GPU の上で認識するところまでを、実際に打って確かめてあります。

## 結果

`cargo run -p xtask -- sherpa-gpu` が、sherpa-onnx v1.13.8 を CUDA 13.4 と ONNX Runtime（ORT）1.31.0 の CUDA 13 版で build し、動かすのに要る共有ライブラリを 1 つの directory に集めます。
`hayamimi-gpu-probe` がその directory のライブラリを使って 1 本の音声（9.6 秒）を開き、認識が終わるまで GPU 用の provider（ORT が計算を GPU へ回す部品）が読み込まれていたことを JSON 1 行で報告します。
`cargo test -p hayamimi-gpu` の RP-17 の test が、その JSON を判定します。

```text
build の流れ
  git clone (v1.13.8) ─┐
  ORT 1.31.0 の archive ┼─> cmake configure/build/install ─> install/lib にライブラリを集める ─> build.json
  CUDA 13.4 / cuDNN 9 ─┘                                                   │
                                                                           v
                              SHERPA_ONNX_LIB_DIR=install/lib ─> cargo build ─> hayamimi-gpu-probe ─> rp17_passed.json
```

図の結論：build は 1 本の xtask で終わり、Rust 側は `SHERPA_ONNX_LIB_DIR` に `install/lib` を渡すだけで繋がります。

## 前提

| 物 | Windows | WSL（Ubuntu 26.04） |
|---|---|---|
| GPU | NVIDIA GeForce RTX 5060 Ti（compute capability 12.0、driver 610.88） | 同じ GPU（WSL の driver 経由） |
| CUDA Toolkit | 13.4（nvcc 13.4.59） | 13.4（redistrib 13.4.1、nvcc 13.4.59）。13.3 は残してある |
| cuDNN | 9.27.0 の CUDA 13 用 archive | 9.27.0.42 の cuda13 |
| ORT | 1.31.0 の `gpu_cuda13`（Microsoft の release の zip） | 1.31.0 の `gpu_cuda13`（Microsoft の release の tgz） |
| sherpa-onnx | v1.13.8（commit `11afbd009a7f8c08f4bcf2fc1b265d0df4670fbf`） | 同じ |
| Rust | 1.99.0 | 1.99.0 |
| 道具 | Visual Studio Build Tools（`dumpbin`）、cmake、Ninja、git、curl、tar | gcc、g++、cmake、Ninja、git、curl、tar、readelf |

全項目は build.json に記録されます。
Windows には CUDA 11.2 と 12.3 も入っていることがあり、PATH の先頭にあるのはその古い `nvcc` です。
そのため xtask は PATH の `nvcc` を使わず、`--cuda-dir` で渡した directory の `bin\nvcc` を使います。

## 手順（Windows）

### 1. build する

Visual Studio の Developer PowerShell を開き、repo の `native` directory で打ちます。
`<...>` は自分の環境の directory に置き換えます。

```powershell
& "<Visual Studio の install 先>\Common7\Tools\Launch-VsDevShell.ps1" -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
cd <repo>\native
cargo run --release -p xtask -- sherpa-gpu --cuda-dir "<CUDA 13.4 の directory>" --cudnn-dir "<cuDNN 9 の CUDA 13 用 archive を展開した directory>"
```

既定の置き場は `native\target\sherpa-gpu` で、git の管理外です。
別の場所に作るときは `--out-dir` を付けます。
版を変えるときは `--sherpa-tag` と `--ort-version` を付けます（既定は v1.13.8 と 1.31.0）。
すでに展開した ORT があるときは `--ort-dir` で渡すと、取得を省きます。

xtask は次の順に 1 段階ずつ進み、失敗したらその段階の名前を言って止まります。

1. sherpa-onnx の指定 tag を浅く clone する。
2. ORT の archive を取り、展開する。
3. cmake で configure、build（並列度 2）、install する。
4. CUDA と cuDNN の共有ライブラリと、Windows では `onnxruntime.lib` を `install\lib` へ集める。
5. 版、ライブラリの SHA-256、provider の対応 GPU 世代を `build.json` へ書く（一時 file に書いて rename するので、途中で止まっても半端な `build.json` は残らない）。

### 2. Rust から繋ぎ、test と probe を打つ

`SHERPA_ONNX_LIB_DIR` に `install\lib` を渡して build します。
`sherpa-onnx-sys` の build script がこの値を読み、`sherpa-onnx-c-api` と `onnxruntime` に link し、その directory の共有ライブラリを `target\<profile>` へ複写します。

```powershell
cd <repo>\native
$env:SHERPA_ONNX_LIB_DIR = "<out-dir>\windows\install\lib"
cargo test --release --target x86_64-pc-windows-msvc -p hayamimi-gpu
```

`rp17_self_built_cuda_library_opens_one_onnxruntime` が、probe を 1 回走らせて判定し、通れば `<out-dir>\windows\rp17_passed.json` に `build.json` の SHA-256 と probe の出力を書きます。
この test は、ReazonSpeech のモデルが `models\sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17` として repo の上の階層のどこかに在ることを前提にします。
probe を単独で打つには次のようにします。

```powershell
$release = "<repo>\native\target\x86_64-pc-windows-msvc\release"
$model = "<モデルの directory>\sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17"
& "$release\hayamimi-gpu-probe.exe" --encoder "$model\encoder-epoch-35-avg-1.onnx" --decoder "$model\decoder-epoch-35-avg-1.onnx" --joiner "$model\joiner-epoch-35-avg-1.onnx" --tokens "$model\tokens.txt" --wav "<repo>\tests\golden\ja\ja_014.wav"
```

出力は JSON 1 行で、読む欄は次の 3 つです。
`loaded_provider` が `cuda`、`cuda_devices` が 1 以上、`onnxruntime_libraries` が 1 であれば、GPU で開けています。
`modules` の各行は、読み込まれたライブラリの名前と、それが実行物と同じ directory から来たか（`exe_directory`）、`System32` から来たか（`system`）、それ以外か（`elsewhere`）を示します。
`hayamimi-gpu-probe --modules` を単独で打つと、モデルを開かずに、その時点で読み込まれているライブラリだけを報告します。

## 手順（WSL）

### 0. 入れる前の戻し方

WSL に入れる物は次の 2 つで、どちらも `/usr/local` の下の新しい directory です。
apt の package は使いません。
`/usr/local/cuda` の alternatives、`PATH`、`/etc/ld.so.conf.d` は変えません。

| 入れる物 | 消す path | 外す設定 |
|---|---|---|
| CUDA Toolkit 13.4（redistrib 13.4.1 の部品） | `/usr/local/cuda-13.4` | なし |
| cuDNN 9.27.0.42（cuda13） | `/usr/local/cudnn-9.27.0.42-cuda13` | なし |
| 取得した archive と build の作業 directory | `~/.cache/hayamimi-r1-wsl-20261014` | なし |

```bash
sudo rm -rf /usr/local/cuda-13.4 /usr/local/cudnn-9.27.0.42-cuda13
rm -rf ~/.cache/hayamimi-r1-wsl-20261014
```

消した後に、`readlink -f /usr/local/cuda` が `/usr/local/cuda-13.3` を指し、`/usr/local/cuda-13.3` が残っていれば、入れる前と同じです。
後から apt の `cuda-toolkit-13-4` を入れたい場合は、`/usr/local/cuda-13.4` が同じ path なので、先に上の 1 行で消します。

### 1. 取得して NVIDIA の値と照合する

NVIDIA の redistrib（部品ごとの配布 archive の一覧）の JSON が、各 archive の相対 path と SHA-256 を持ちます。
`sha256sum -c` が全部 `OK` になることを確かめてから展開します。

```bash
W=~/.cache/hayamimi-r1-wsl-20261014
mkdir -p $W/redist && cd $W/redist
curl -sS -O https://developer.download.nvidia.com/compute/cuda/redist/redistrib_13.4.1.json
curl -sS -O https://developer.download.nvidia.com/compute/cudnn/redist/redistrib_9.27.0.json
jq -r 'to_entries[] | select(.key|test("^(cuda_nvcc|cuda_cudart|cuda_cuobjdump|libcublas|libcufft|libcurand|cuda_nvrtc|libnvjitlink|cuda_crt)$")) | .value["linux-x86_64"] | "\(.relative_path) \(.sha256)"' redistrib_13.4.1.json > cuda.list
jq -r '.cudnn["linux-x86_64"].cuda13 | "\(.relative_path) \(.sha256)"' redistrib_9.27.0.json > cudnn.list
while read -r rel sum; do f=$(basename $rel); curl -L --fail -sS -o $f https://developer.download.nvidia.com/compute/cuda/redist/$rel; echo "$sum  $f"; done < cuda.list > cuda.sha256
while read -r rel sum; do f=$(basename $rel); curl -L --fail -sS -o $f https://developer.download.nvidia.com/compute/cudnn/redist/$rel; echo "$sum  $f"; done < cudnn.list > cudnn.sha256
sha256sum -c cuda.sha256 cudnn.sha256
```

CUDA は 13.4.1 に固定してあります。
Windows の toolkit の nvcc（13.4.59）と同じ版で、2 つの OS の build を比べるときに副版が揃うからです。
redistrib には 13.4.2 と 13.4.3 もあり、nvcc は 13.4.92 です。
上げるかどうかは別に決めます。

### 2. 展開する

```bash
cd ~/.cache/hayamimi-r1-wsl-20261014/redist
sudo mkdir -p /usr/local/cuda-13.4 /usr/local/cudnn-9.27.0.42-cuda13
for f in cuda_*.tar.xz libc*.tar.xz libnv*.tar.xz; do sudo tar -xJf $f -C /usr/local/cuda-13.4 --strip-components=1; done
sudo tar -xJf cudnn-linux-x86_64-9.27.0.42_cuda13-archive.tar.xz -C /usr/local/cudnn-9.27.0.42-cuda13 --strip-components=1
/usr/local/cuda-13.4/bin/nvcc --version
```

`nvcc --version` が `release 13.4, V13.4.59` を出せば成功です。

### 3. build して test を打つ

ORT の tgz も、GitHub の release の値と照合します（1.31.0 の Linux の tgz は `2867dec99c6a4926e8142361b9d4d1518c16c85b854b4d41d28d65d3b2c81135`、244,574,938 bytes）。
xtask は `<out-dir>/downloads` に同じ名前の file が在ればそれを使うので、手で取って照合してから置きます。
`--out-dir` を Windows の drive の上に置くと遅いので、WSL の側の directory にします。

```bash
W=~/.cache/hayamimi-r1-wsl-20261014
mkdir -p $W/downloads
curl -L --fail -sS -o $W/downloads/onnxruntime-linux-x64-gpu_cuda13-1.31.0.tgz https://github.com/microsoft/onnxruntime/releases/download/v1.31.0/onnxruntime-linux-x64-gpu_cuda13-1.31.0.tgz
sha256sum $W/downloads/onnxruntime-linux-x64-gpu_cuda13-1.31.0.tgz
cd <repo>/native
cargo run --release --target-dir $W/target -p xtask -- sherpa-gpu --cuda-dir /usr/local/cuda-13.4 --cudnn-dir /usr/local/cudnn-9.27.0.42-cuda13 --out-dir $W
export SHERPA_ONNX_LIB_DIR=$W/linux/install/lib
cargo build --release --target-dir $W/target -p hayamimi-gpu
LD_LIBRARY_PATH=$W/target/release cargo test --release --target-dir $W/target -p hayamimi-gpu
```

`LD_LIBRARY_PATH` に実行物の directory を置くのは、ORT の CUDA provider（`libonnxruntime_providers_cuda.so`）が `libcublas.so.13` などを名前だけで探し、provider 自身には探す場所の指定が無いためです。
置かないと `/usr/local/cuda`（13.3）の物が先に当たります。
RP-17 は CUDA の主版（13）しか見ないので、13.3 でも通りますが、13.4 の物を使った証拠にはなりません。

## 実測した結果

### ORT 1.31.0（現行）

RTX 5060 Ti で `ja_014.wav`（9600 ms）を、build ごとに 4 回続けて開いた結果です（同じ音声、同じ build、cache の状態は揃えていません）。

| 欄 | Windows | WSL |
|---|---|---|
| `loaded_provider` | `cuda`（4 回とも） | `cuda`（4 回とも） |
| `cuda_devices` | 1 | 1 |
| `ort_version` | `1.31.0` | `1.31.0` |
| `onnxruntime_libraries` | 1 | 1 |
| `decode_ms` | 2469、678、651、660 | 952、529、552、553 |
| `decoded_chars` | 44 | 44 |

どちらも 1 回目だけが遅く、2 回目以降は 1 割から 2 割の幅に収まっています。
1 回目の遅れが初期化なのか、PTX から機械語への変換なのかは切り分けていません（未検証）。
GPU と CPU の速さの比較は、音声の長さ、warm-up、反復を揃えた別の測定で行います。

Windows で認識が終わるまで読み込まれていたライブラリのうち、CUDA と ORT に関わる物は次のとおりです。

- 実行物と同じ directory：`onnxruntime`、`onnxruntime_providers_cuda`、`onnxruntime_providers_shared`、`sherpa-onnx-c-api`、`cublas64_13`、`cublaslt64_13`、`cudnn64_9`、`cudnn_graph64_9`、`cudnn_ops64_9`、`cudnn_heuristic64_9`、`cudnn_engines_precompiled64_9`、`cudnn_engines_runtime_compiled64_9`、`cudnn_engines_tensor_ir64_9`
- NVIDIA の driver 側：`nvcuda`、`nvcuda64`、`nvcudart_hybrid64`、`nvapi64`、`nvapi64_impl`、`nvdxgdmal64`、`nvobjectloader64`

Windows では `cudart64_12.dll` も `cudart64_13.dll` も読み込まれていません。
`dumpbin /dependents onnxruntime_providers_cuda.dll` の依存は `cublasLt64_13.dll`、`cublas64_13.dll`、`onnxruntime_providers_shared.dll`、KERNEL32 と CRT だけです。
CUDA runtime はこの provider に静的に含まれていると読めますが、含まれていると確かめる検査は打っていません。
WSL では `libcudart.so.13` が読み込まれ（provider の `readelf -d` の NEEDED に在ります）、`libcudart.so.12` は読み込まれていません。

CUDA の版は、provider の library の中の文字列では判別できません。
Windows の provider には `cudart64_12.dll` と `cudart64_13.dll` の文字列が各 1 件在ります。
RP-17 は、読み込まれたライブラリの名前で判定します。
`cublas64_13` と `cublasLt64_13`（WSL は `libcublas.so.13` と `libcublasLt.so.13`）が各 1 本、`cudart64_12`、`cublas64_12`、`cublasLt64_12`（WSL は `libcudart.so.12`、`libcublas.so.12`、`libcublasLt.so.12`）が 0 本で、`onnxruntime` が実行物と同じ directory から来ていれば通ります。
`cufft64_12.dll` は CUDA 13.4 の toolkit 自身が持つので、在っても通ります。
名前は大文字と小文字を区別せずに数えます。

provider の GPU 向け機械語は、`cuobjdump --list-elf` で次のとおりです。

| build | `sm_120a` の機械語 | `sm_120` の機械語 | `sm_120` の PTX |
|---|---|---|---|
| Windows、ORT 1.31.0 | 279 | 0 | 38 |
| WSL、ORT 1.31.0 | 317 | 0 | 0 |
| Windows、ORT 1.28.2 | 203 | 0 | 235 |

PTX は実行時に GPU 向けへ変換される中間表現です。
RTX 5060 Ti（compute capability 12.0）で `sm_120a` の機械語が実際に使われているかは、確かめていません（ASSUMPTION）。
全 kernel が機械語で当たるとも言えません。
`sm_120` の PTX から実行時に変換されて動く kernel が在るかどうかも、確かめていません（ASSUMPTION）。
RP-24 の `sm_120` 系の判定は、`sm_120` か `sm_120a` の機械語が 1 件以上在れば真なので、3 つとも真です。
Windows と WSL の `build.json` を RP-24 の比較（`windows_and_wsl_builds_match`）に掛けると、5 項目が一致します（2 つの `build.json` を `native/target/sherpa-gpu/{windows,linux}/` に写して打った）。

### ORT 1.28.2（退路）

ORT 1.28.2 の build は `native/target/sherpa-gpu` に残してあり、消していません。
Windows で同じ音声を 4 回開いた結果は、`loaded_provider` が `cuda`、ORT が `1.28.2`、`onnxruntime_libraries` が 1、`decoded_chars` が 44、`decode_ms` が 660、707、658、630 でした。
これより前に `decode_ms` が 2490 の実行が 1 回ありました。
その実行は module の snapshot を認識器の解放後に取っていたため `loaded_provider` が `cpu` と出ましたが、その表示は誤りで、実際にどの provider で動いたかは分かりません。

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
`sherpa-onnx-c-api.dll` が名前だけで `onnxruntime.dll` を import すると、実行物の隣の物ではなくこの 1.17.1 に結びつく場合があります。
そのとき、起動は次の文言で異常終了します（終了コード -1073741819）。

```text
The requested API version [28] is not available ... Current ORT Version is: 1.17.1
```

実行ごとに結びつく先が変わる場合があり、どの条件で System32 側に結びつくかは突き止めていません。
`.local` file による探索順の変更も試しましたが、効きませんでした。

hayamimi-gpu は次の 2 つで防いでいます。

1. 実行物を link するときに `sherpa-onnx-c-api.dll` を遅延読み込みにする（`native/hayamimi-gpu/build.rs` が `/DELAYLOAD:sherpa-onnx-c-api.dll` と `delayimp.lib` を、binaries と integration test の両方に渡す）。
2. 最初の sherpa-onnx の呼び出しの前に、実行物の directory の `onnxruntime.dll` を完全な path で読み込む（`runtime::preload_beside_executable`）。

先に実行物の隣の DLL が読み込まれていれば、後から来る `onnxruntime.dll` の import はその読み込み済みの物で解決されます。

`hayamimi-gpu` に依存する crate は、自分の build script に同じ 2 つの link 引数を、`bins` と `tests` の両方に置く必要があります。
library の crate は、依存する側の link 引数を決められないためです。
RP-17 の test が `cargo metadata` で依存する crate を数え、各 crate の build script を読んで、2 つの引数が `bins` と `tests` の両方に在るかを確かめます（現在の workspace では `hayamimi-gpu` 自身だけです）。

test の実行物は cargo が `target/<triple>/<profile>/deps/` に置きますが、`sherpa-onnx-sys` が共有ライブラリを複写するのは `target/<triple>/<profile>/` です。
test の process の中で認識を走らせて「実行物の隣」の `onnxruntime.dll` を読み込もうとすると、`deps/` に無いので `LoadLibraryExW failed` で止まります（実測）。
そのため RP-17 の test は、認識を test の process の中では走らせず、`hayamimi-gpu-probe` を子の process として走らせ、その JSON を判定します。
認識を test の process の中で走らせる crate（今後の認識の test）は、この点を解く必要があります（未解決）。

### Linux では ORT を先読みしない

WSL では、`sherpa-onnx-sys` が `libonnxruntime.so`、`libonnxruntime.so.1`、`libonnxruntime.so.1.31.0` を別々の file として複写します（symlink ではなく複写）。
そこへ `libonnxruntime.so` を完全な path で先読みすると、`libsherpa-onnx-c-api.so` が名前（SONAME）で要求する `libonnxruntime.so.1` が別の file として読み込まれ、ORT が 2 つ入りました（`onnxruntime_libraries` が 2。RP-17 が捕まえる形）。
そこで Linux では先読みをせず、`libsherpa-onnx-c-api.so` の RUNPATH（`$ORIGIN`）に任せて、版の問い合わせも SONAME の `libonnxruntime.so.1` で行います。
Linux には `System32` に当たる物が無く、先読みの目的がありません。

## 採らない手段

採ったのは、利用者が指定した「sherpa-onnx の C ライブラリを CUDA 13.4 と ORT の `gpu_cuda13` で自分で build し、`SHERPA_ONNX_LIB_DIR` で繋ぐ」形です。
次の手段は、理由とともに採りません。

- **CUDA 12 版の build を主にすること**：sherpa-onnx の既定は CUDA 12 版の ORT ですが、CUDA 13.4 の環境があるので使いません。CUDA 13 版が動かないときの退避先としてだけ残してあり、手順は設計の 3.12 にあります。
- **ORT を 1.28.2 のまま測定へ進むこと**：`ort` crate の既定の API 版が 1.31 で、1.28.2 では api-28 を指定する外れ道を通ることになり、測定を後で測り直す羽目になるため、1.31.0 に上げました。1.28.2 の build は退路として残してあります。`hayamimi-gpu` 自身は `ort` crate に依存していません。
- **公開済みの CUDA 版を取ること**：`sherpa-onnx-sys` の build script は CUDA 版を自分では取りません。版と option を固定して再現するために、自分で build します。
- **cloud の認識 API と実行時の Python**：速度を最優先にするため使いません。
- **provider の library の文字列で CUDA の版を判別すること**：両方の主版の文字列が在るため、判別できません。
- **`native/hayamimi-eval` の変更**：この build と接続は触りません。

## 戻し方

作った物は、`--out-dir` の下にだけ在ります（Windows の既定は `native\target\sherpa-gpu`、WSL は作業 directory）。
消すと build 前に戻り、他の物は変わりません。

```powershell
Remove-Item -Recurse -Force "<repo>\native\target\sherpa-gpu"
```

ORT 1.31.0 の Windows の build は `native\target\sherpa-gpu-ort131` に、1.28.2 の build は `native\target\sherpa-gpu` に在り、どちらも同じ方法で消せます。
WSL に入れた CUDA と cuDNN の戻し方は、「手順（WSL）」の「0. 入れる前の戻し方」にあります。

Rust 側を CPU 版に戻すには、`SHERPA_ONNX_LIB_DIR` を設定せずに build します。
build script が CPU 版の archive を取ります（設計 3.12）。
CUDA 12 版へ戻す手順は、設計の 3.12 に書いてあります。

## 未検証

- 認識結果の文字数が 44 で、golden の期待値（45 文字）と 1 文字違います。文字そのものは比べていません。
- System32 の DLL に結びつく条件。上の遅延読み込みと事前読み込みは、probe の実行で防げたことだけを確かめています。
- 1 回目の `decode_ms` が遅い理由（初期化か、PTX の変換か）と、`sm_120a` の機械語が実際に使われているか。
- 認識を test の process の中で走らせるときの「実行物の隣」の解き方（上記）。
- RP-24 の「同じ OS で 2 回 build して一致する」（`two_builds_on_one_os_match`）は、2 回目の build を作っていないので打っていません。Windows と WSL の比較（`windows_and_wsl_builds_match`）は、2 つの `build.json` を写して打ちました。
- WSL で `LD_LIBRARY_PATH` を置かずに打った場合に、CUDA 13.3 と 13.4 のどちらの物が読み込まれるか（13.3 が先に当たると想定していますが、読み込み元の path は probe の出力に出ません）。
- CUDA の redistrib 13.4.2 と 13.4.3 への移行。
