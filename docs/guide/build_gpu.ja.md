# GPU 版 sherpa-onnx の build と接続

Rust 版の認識を NVIDIA の GPU（CUDA）で動かすために、sherpa-onnx の C ライブラリを自分で build し、Rust の crate をそこへ繋ぐ手順です。
Windows と WSL（Linux）の両方で、1 本の音声を GPU の上で認識するところまでを、実際に打って確かめてあります。
v2.0.1 と ORT 1.31.1 の build は、2026-10-29 に WSL で打って確かめました。
RP-17 は 6 本とも通りました（`rp17_passed.json` の `loaded_provider` は `cuda`、ORT は 1.31.1、`onnxruntime_libraries` は 1、9.6 秒の音声から `decoded_chars` が 44。`decode_ms` は run ごとに動き、記録の値は 1743、独立検証の席の run は 674）。
Windows の build は 2026-10-29 に v2.0.1 と ORT 1.31.1 で打ち直し、build と `hayamimi-gpu` の lib test と RP-17 まで通しました（H169）。crate は `=2.0.1` なので、Windows の `SHERPA_ONNX_LIB_DIR` を v1.13.8 の library のままにして build すると、`SherpaOnnxSpeechSegment` の `start` と、`SherpaOnnxCircularBufferHead` の戻りと `SherpaOnnxCircularBufferGet` の開始位置の型（v1.13.8 は `int32`、v2.0.1 は `int64`）が合わない組になり、誤った値を読むおそれがあります（打っていません）。この組は、sherpa-onnx の object を作る前に `hayamimi-gpu` の `library::require()` が止めます（C library の `sherpa_onnx::version()` が `2.0.1` でなければ `GpuError::Library`。H168。v1.13.8 の library を `LD_LIBRARY_PATH` に置いた WSL の実走で止まることを確かめ、Windows でも、`.venv` の Python の wheel が持つ 1.13.6 の library を PATH の先頭に置いた実走で止まることを確かめました）。Windows でも、C library を v2.0.1 で作り直した directory を指してから build します。

## 結果

`cargo run -p xtask -- sherpa-gpu` が、sherpa-onnx v2.0.1 を CUDA 13.4 と ONNX Runtime（ORT）1.31.1 の CUDA 13 版で build し、動かすのに要る共有ライブラリを 1 つの directory に集めます。
`hayamimi-gpu-probe` がその directory のライブラリを使って 1 本の音声（9.6 秒）を開き、認識が終わるまで GPU 用の provider（ORT が計算を GPU へ回す部品）が読み込まれていたことを JSON 1 行で報告します。
`cargo test -p hayamimi-gpu` の RP-17 の test が、その JSON を判定します。

```text
build の流れ
  git clone (v2.0.1) ─┐
  ORT 1.31.1 の archive ┼─> cmake configure/build/install ─> install/lib にライブラリを集める ─> build.json
  CUDA 13.4 / cuDNN 9 ─┘                                                   │
                                                                           v
                              SHERPA_ONNX_LIB_DIR=install/lib ─> cargo build ─> hayamimi-gpu-probe ─> rp17_passed.json
```

図の結論：build は 1 本の xtask で終わり、Rust 側は `SHERPA_ONNX_LIB_DIR` に `install/lib` を渡すだけで繋がります。

## 前提

| 物 | Windows | WSL（Ubuntu 26.04） |
|---|---|---|
| GPU | NVIDIA GeForce RTX 5060 Ti（compute capability 12.0、driver 610.88） | 同じ GPU（WSL の driver 経由） |
| CUDA Toolkit | 13.4.3（redistrib、nvcc 13.4.92）。更新前の 13.4（installer、nvcc 13.4.59）は残してある | 13.4.3（redistrib、nvcc 13.4.92）。更新前の 13.4.1（nvcc 13.4.59）と 13.3 は残してある |
| cuDNN | 9.27.0 の CUDA 13 用 archive | 9.27.0.42 の cuda13 |
| ORT | 1.31.1 の `gpu_cuda13`（Microsoft の release の zip） | 1.31.1 の `gpu_cuda13`（Microsoft の release の tgz） |
| sherpa-onnx | v2.0.1（commit `af2d8d81dbfa61c4cea4c568da78f3982edca005`） | v2.0.1（同じ commit） |
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
版を変えるときは `--sherpa-tag` と `--ort-version` を付けます（既定は v2.0.1 と 1.31.1）。
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
| CUDA Toolkit 13.4.3（redistrib の部品） | `/usr/local/cuda-13.4.3` | なし |
| CUDA Toolkit 13.4.1（更新前。redistrib の部品） | `/usr/local/cuda-13.4` | なし |
| cuDNN 9.27.0.42（cuda13） | `/usr/local/cudnn-9.27.0.42-cuda13` | なし |
| build の出力（4 本）と redistrib の照合表（`redist-sums`。下の表） | `~/.local/share/hayamimi/build` | なし |

WSL の作業 directory は 5 本あり、build の 3 本（`cuda1343`、`cuda1343-repeat`、`cuda1341`）は `native/target` の下の symlink から指され、現行の `cuda1343-sherpa201` は symlink を持たず、`SHERPA_ONNX_LIB_DIR` で指します。同じ `~/.local/share/hayamimi/build` には、この表の外に、ORT 1.28.3 の退路の build（`ort1283`。設計 `rust_port.ja.md` の環境の表）と、その RP-17 を打った作り直せる cargo の target（`ort1283-target`）、alsa の pkg-config と `libasound.so` の symlink を置いた `alsa-shim` もあります。
`~/.cache` には何も残しません（下の手順の作業 directory `W` は、取得と build の間だけ使います）。
取得した archive と cargo の `target` は作り直せるので残さず、redistrib の部品の sha256 の照合表だけを `redist-sums` に残します。

| WSL の作業 directory | 中身 | 指す symlink | 消すと |
|---|---|---|---|
| `~/.local/share/hayamimi/build/cuda1343-sherpa201` | 現行（v2.0.1 と ORT 1.31.1、CUDA 13.4.3）の build（`linux` の下に `build.json` と `install`、ほかに `ort`、`src-v2.0.1`、`downloads`） | なし（`SHERPA_ONNX_LIB_DIR` で指す） | 現行の build と probe が使えなくなる |
| `~/.local/share/hayamimi/build/cuda1343` | v1.13.8 と ORT 1.31.0 の build（CUDA 13.4.3。WSL の退路。`linux` の下に `build.json` と `install`） | `native/target/sherpa-gpu/linux`（`cuda1343/linux` を指す） | 退路の `sherpa-gpu/linux` が切れる |
| `~/.local/share/hayamimi/build/cuda1343-repeat` | v1.13.8 の 2 回目の build | `native/target/sherpa-gpu-repeat/linux`（`cuda1343-repeat/linux` を指す） | `two_builds_on_one_os_match` の実走が読めなくなる |
| `~/.local/share/hayamimi/build/cuda1341` | 更新前（13.4.1）の build（directory の直下に `build.json` と `install`） | `native/target/sherpa-gpu-cuda1341/linux`（`cuda1341` を指す） | 退路の 13.4.1 の build が消える |
| `~/.local/share/hayamimi/build/redist-sums` | CUDA 13.4.1、13.4.3、cuDNN 9.27.0.42 の redistrib の部品の一覧と sha256（`*.list`、`*.sha256`） | なし | 入れ直すときに照合の表を取り直すことになる |

現行を残して片付けるときは、`cuda1343-repeat` だけを消します。
`cuda1343` を消すと、WSL の退路（v1.13.8）が消えます。`cuda1343-sherpa201` を消すと、現行の build と probe が使えなくなります。
全部を戻すときは、5 本とも消します。

```bash
sudo rm -rf /usr/local/cuda-13.4.3 /usr/local/cuda-13.4 /usr/local/cudnn-9.27.0.42-cuda13
rm -rf ~/.local/share/hayamimi/build
rm native/target/sherpa-gpu/linux native/target/sherpa-gpu-repeat/linux native/target/sherpa-gpu-cuda1341/linux
```

最後の行は symlink だけを消します（`rm -r` を付けないので、指す先は消えません）。

cmake の出力（`build/CMakeCache.txt` など）、`install/sherpa-onnx.pc`、`install/lib` の共有 library の RUNPATH の 2 つ目には、build した時の作業 directory の path が文字として残っています。
共有 library の RUNPATH は `$ORIGIN` が先なので、読み込みは `~/.local/share/hayamimi/build` の下の directory で足ります（`ldd target/release/hayamimi-eval` が `target/release` の library だけを指すことで確かめました）。

消した後に、`readlink -f /usr/local/cuda` が `/usr/local/cuda-13.3` を指し、`/usr/local/cuda-13.3` が残っていれば、入れる前と同じです。
後から apt の `cuda-toolkit-13-4` を入れたい場合は、`/usr/local/cuda-13.4` が同じ path なので、先に消します。

### 1. 取得して NVIDIA の値と照合する

NVIDIA の redistrib（部品ごとの配布 archive の一覧）の JSON が、各 archive の相対 path と SHA-256 を持ちます。
`sha256sum -c` が全部 `OK` になることを確かめてから展開します。

```bash
W=~/.cache/hayamimi-r1-wsl-20261014
mkdir -p $W/redist && cd $W/redist
curl -sS -O https://developer.download.nvidia.com/compute/cuda/redist/redistrib_13.4.3.json
curl -sS -O https://developer.download.nvidia.com/compute/cudnn/redist/redistrib_9.27.0.json
jq -r 'to_entries[] | select(.key|test("^(cuda_nvcc|cuda_cudart|cuda_cuobjdump|libcublas|libcufft|libcurand|cuda_nvrtc|libnvjitlink|cuda_crt)$")) | .value["linux-x86_64"] | "\(.relative_path) \(.sha256)"' redistrib_13.4.3.json > cuda.list
jq -r '.cudnn["linux-x86_64"].cuda13 | "\(.relative_path) \(.sha256)"' redistrib_9.27.0.json > cudnn.list
while read -r rel sum; do f=$(basename $rel); curl -L --fail -sS -o $f https://developer.download.nvidia.com/compute/cuda/redist/$rel; echo "$sum  $f"; done < cuda.list > cuda.sha256
while read -r rel sum; do f=$(basename $rel); curl -L --fail -sS -o $f https://developer.download.nvidia.com/compute/cudnn/redist/$rel; echo "$sum  $f"; done < cudnn.list > cudnn.sha256
sha256sum -c cuda.sha256 cudnn.sha256
```

CUDA は 13.4 系の最新（13.4.3、redistrib の release_date 2026-10-13）にしてあり、Windows と WSL で同じ版です。
更新前は Windows が installer の 13.4（nvcc 13.4.59）、WSL が redistrib の 13.4.1（nvcc 13.4.59）でした。
上げるかどうかは別に決めます。

### 2. 展開する

```bash
cd ~/.cache/hayamimi-r1-wsl-20261014/redist
sudo mkdir -p /usr/local/cuda-13.4.3 /usr/local/cudnn-9.27.0.42-cuda13
for f in cuda_*.tar.xz libc*.tar.xz libnv*.tar.xz; do sudo tar -xJf $f -C /usr/local/cuda-13.4.3 --strip-components=1; done
sudo tar -xJf cudnn-linux-x86_64-9.27.0.42_cuda13-archive.tar.xz -C /usr/local/cudnn-9.27.0.42-cuda13 --strip-components=1
/usr/local/cuda-13.4.3/bin/nvcc --version
```

`nvcc --version` が `release 13.4, V13.4.92` を出せば成功です。

### 3. build して test を打つ

ORT の tgz も、GitHub の release の値と照合します（1.31.1 の Linux の tgz は `b2146862e691f7e338f5aff9c7a9a5567c2ca57cf01442a1adb3a13bf62d9095`、244,573,709 bytes。1.31.0 の tgz は `2867dec99c6a4926e8142361b9d4d1518c16c85b854b4d41d28d65d3b2c81135`、244,574,938 bytes）。
xtask は `<out-dir>/downloads` に同じ名前の file が在ればそれを使うので、手で取って照合してから置きます。
`--out-dir` を Windows の drive の上に置くと遅いので、WSL の側の directory にします。

```bash
W=~/.cache/hayamimi-r1-wsl-20261014
OUT=~/.local/share/hayamimi/build/cuda1343-sherpa201
mkdir -p $OUT/downloads
curl -L --fail -sS -o $OUT/downloads/onnxruntime-linux-x64-gpu_cuda13-1.31.1.tgz https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-linux-x64-gpu_cuda13-1.31.1.tgz
sha256sum $OUT/downloads/onnxruntime-linux-x64-gpu_cuda13-1.31.1.tgz
cd <repo>/native
cargo run --release --target-dir $W/target -p xtask -- sherpa-gpu --cuda-dir /usr/local/cuda-13.4.3 --cudnn-dir /usr/local/cudnn-9.27.0.42-cuda13 --out-dir $OUT
export SHERPA_ONNX_LIB_DIR=$OUT/linux/install/lib
cargo build --release --target-dir $W/target -p hayamimi-gpu
cargo test --release --target-dir $W/target -p hayamimi-gpu
```

`LD_LIBRARY_PATH` は要りません。
`hayamimi-gpu` の `build.rs` が、実行物に `$ORIGIN`（実行物と同じ directory）の run path を付けるからです（test の実行物には `$ORIGIN/..` も付けます）。
run path は `RUNPATH` ではなく `RPATH`（`-Wl,--disable-new-dtags`）で書いてあります。ORT の CUDA provider（`libonnxruntime_providers_cuda.so`）は `libcublas.so.13` と `libcudnn.so` を名前だけで探し、provider 自身には探す場所の指定が無いので、実行物の `RUNPATH` は provider が開く library に届かないためです。
`RUNPATH` だけにして `LD_LIBRARY_PATH` を外して打つと（実測）、実行物は起動しますが、provider の `libcublas.so.13` と `libcudart.so.13` が `/usr/local/cuda-13.3` から読まれ、`libcudnn.so` は見つからず、`cuDNN is unavailable` で abort しました。
`RPATH` にすると、`/proc/<pid>/maps` で `libcublas.so.13`、`libcublasLt.so.13`、`libcudart.so.13`、`libcurand.so.10`、`libcudnn.so.9`、`libonnxruntime*` がすべて実行物の directory（13.4 から複写した物）から読まれ、13.3 の path は 1 つも出ませんでした。
実行物の directory に置く CUDA の library は、`sherpa-onnx-sys` が `install/lib` から複写した物です。

`hayamimi-gpu` に依存する crate の実行物にも、同じ run path が要ります。

## CUDA を 13.4.3 に上げる手順と戻し方

更新前の版は、Windows が installer で入れた 13.4（nvcc 13.4.59。`C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.4`）、WSL が redistrib の 13.4.1（nvcc 13.4.59。`/usr/local/cuda-13.4`）です。
更新では、どちらの OS でも元の directory に手を付けず、13.4.3 を別の directory に並べました。
Windows は installer も winget も使わず、WSL と同じ redistrib の zip を使っています。installer は `v13.4` を上書きして元に戻せなくなり、redistrib は NVIDIA の json が SHA-256 を持つためです。

Windows の 13.4.3 は `%USERPROFILE%\cuda-redist\v13.4.3` に置きます。WSL から打った command です（`<Windows の user directory>` は `/mnt/c/Users/...`）。

```bash
D=<Windows の user directory>/cuda-redist
mkdir -p $D/downloads $D/v13.4.3 $D/stage && cd $D/downloads
curl -sS -O https://developer.download.nvidia.com/compute/cuda/redist/redistrib_13.4.3.json
jq -r 'to_entries[] | select(.key|test("^(cuda_nvcc|cuda_cudart|cuda_cuobjdump|libcublas|libcufft|libcurand|cuda_nvrtc|libnvjitlink|cuda_crt)$")) | .value["windows-x86_64"] | "\(.relative_path) \(.sha256)"' redistrib_13.4.3.json > cuda.list
while read -r rel sum; do f=$(basename $rel); curl -L --fail -sS -o $f https://developer.download.nvidia.com/compute/cuda/redist/$rel; echo "$sum  $f"; done < cuda.list > cuda.sha256
sha256sum -c cuda.sha256
for z in *.zip; do rm -rf $D/stage/*; unzip -q $z -d $D/stage && cp -a $D/stage/*/. $D/v13.4.3/; done
$D/v13.4.3/bin/nvcc.exe --version
```

`nvcc --version` が `V13.4.92` を出せば成功です。
WSL の 13.4.3 は、上の「手順（WSL）」の 1 と 2 のとおりです（`/usr/local/cuda-13.4.3`）。

上げた後は、`--cuda-dir` を新しい directory にして xtask を打ち直し、`cargo test`、`clippy`、`fmt` を両 OS で打ち直します。

`sherpa-onnx-sys` の build script は、`SHERPA_ONNX_LIB_DIR` の共有ライブラリを `target/<profile>` へ複写するだけで、古い版の file を消しません。
同じ `target` を使い回すと、前の版の DLL と `.so` が残ります（Windows の `ort131-build` の release には 13.4.1 の DLL が残っていました。WSL の `target/release` には `libcublas.so.13.7.0.27` が残っています）。
`libcublas.so.13` などの名前の file は上書きされるので run は新しい版を読みますが、残りは紛らわしいので、版を変えたら `--target-dir` の directory を消して作り直すか、`target/<profile>` の共有ライブラリを消して build し直します。

```powershell
cargo run --release -p xtask -- sherpa-gpu --cuda-dir "%USERPROFILE%\cuda-redist\v13.4.3" --cudnn-dir "<cuDNN 9 の CUDA 13 用 archive を展開した directory>"
```

更新前の build は、`native/target/sherpa-gpu-cuda1341`（Windows。`linux` は WSL の `~/.local/share/hayamimi/build/cuda1341` への symlink）に残してありました。2026-10-29 に Windows の build を作り直した日の `native/target` には、この directory も `sherpa-gpu-repeat` も無く（`sherpa-gpu` だけ。H169）、WSL の `cuda1341` は在りました。

**13.4.1 に戻すとき**：xtask の `--cuda-dir` を元の directory（Windows は installer の `v13.4`、WSL は `/usr/local/cuda-13.4`）に戻して build し直すか、残してあった `sherpa-gpu-cuda1341` の `windows\install\lib` を `SHERPA_ONNX_LIB_DIR` にして繋ぎ直します（今の木には無いので、Windows は `--cuda-dir` を戻して build し直します）。
13.4.3 の directory は次のように消します。

```bash
sudo rm -rf /usr/local/cuda-13.4.3
rm -rf <Windows の user directory>/cuda-redist
```

## 実測した結果

### ORT 1.31.0（2026-10-29 に Windows も 1.31.1 に上げる前の測定）

RTX 5060 Ti で `ja_014.wav`（9600 ms）を、build ごとに 4 回続けて開いた結果です（同じ音声、同じ build、cache の状態は揃えていません）。
CUDA 13.4.1 の build（更新前）と、13.4.3 の build（ORT 1.31.0 の時の現行）の両方を載せます。

| 欄 | Windows、13.4.1 | WSL、13.4.1 | Windows、13.4.3 | WSL、13.4.3 |
|---|---|---|---|---|
| `loaded_provider` | `cuda` | `cuda` | `cuda` | `cuda` |
| `cuda_devices` | 1 | 1 | 1 | 1 |
| `ort_version` | `1.31.0` | `1.31.0` | `1.31.0` | `1.31.0` |
| `onnxruntime_libraries` | 1 | 1 | 1 | 1 |
| `decode_ms`（4 回） | 2469、678、651、660 | 952、529、552、553 | 731、658、680、698 | 621、609、588、569 |
| `decoded_chars` | 44 | 44 | 44 | 44 |

13.4.3 の WSL の cuBLAS は 13.8.0.4 で、実行物の directory の `libcublas.so.13` の SHA-256 が `/usr/local/cuda-13.4.3/lib/libcublas.so.13.8.0.4` と一致します。
13.4.1 の cuBLAS は 13.7.0.27 です。

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
| WSL、ORT 1.28.3（退路） | 203 | 0 | 235 |

PTX は実行時に GPU 向けへ変換される中間表現です。
RTX 5060 Ti（compute capability 12.0）で `sm_120a` の機械語が実際に使われているかは、確かめていません（ASSUMPTION）。
全 kernel が機械語で当たるとも言えません。
`sm_120` の PTX から実行時に変換されて動く kernel が在るかどうかも、確かめていません（ASSUMPTION）。
RP-24 の `sm_120` 系の判定は、`sm_120` か `sm_120a` の機械語が 1 件以上在れば真なので、3 つとも真です。
RP-24 の実走は、`native/target/sherpa-gpu/{windows,linux}/`（比べる組。打った時は、`linux` が WSL の作業 directory `cuda1343` への symlink で、`windows` と同じ v1.13.8 と ORT 1.31.0 の build でした）を読み、`windows_and_wsl_builds_match` が 5 項目の一致を確かめます。
同じ OS の 2 回目の build は `native/target/sherpa-gpu-repeat/<os>/` に作り、`two_builds_on_one_os_match` が `sherpa-gpu/<os>/` と比べます。
実走の 2 つは `cargo test --release -p xtask --test rp24 -- --ignored` で打ちます。どちらも通りました（Windows と WSL）。Windows の build を v2.0.1 と ORT 1.31.1 に作り直した後は、`linux` の symlink も 2 回目の build も作っておらず、RP-24 は v2.0.1 で打ち直していません。

### ORT 1.28.3（退路）

退路の ORT は 1.28.3 です。両 OS とも Microsoft の asset（`onnxruntime-win-x64-gpu_cuda13-1.28.3.zip`、`onnxruntime-linux-x64-gpu_cuda13-1.28.3.tgz`）を使います。
WSL では、現行と同じ手順に `--ort-version 1.28.3` と別の `--out-dir` を足して build しました。

```bash
OUT=~/.local/share/hayamimi/build/ort1283
cd <repo>/native
cargo run --release -p xtask -- sherpa-gpu --cuda-dir /usr/local/cuda-13.4.3 --cudnn-dir /usr/local/cudnn-9.27.0.42-cuda13 --ort-version 1.28.3 --out-dir $OUT
mkdir -p target/sherpa-gpu-ort1283 && ln -s $OUT/linux target/sherpa-gpu-ort1283/linux
SHERPA_ONNX_LIB_DIR=$OUT/linux/install/lib cargo test --release -p hayamimi-gpu --test rp17
```

`build.json` の ORT の版は 1.28.3、ORT の archive の sha256 は `33e91f819324449bede2d1e853c8d46bb6253fa8890bac98ae676f041c8f73a6` で、GitHub の release が書く digest と一致しました。
RP-17 は 6 本とも通りました。`rp17_passed.json` の `loaded_provider` は `cuda`、ORT は `1.28.3`、`onnxruntime_libraries` は 1、`decoded_chars` は 44、`build_json_sha256` は `build.json` の `sha256sum` と一致します。
`decode_ms` は、build の直後の 1 回目が 2430、2 回目が 1160 でした。2 回では分布は分からず、遅れた理由（初期化か、PTX の変換か）も分けていません。
退路の build は現行の `native/target/sherpa-gpu` とは別の directory に在り、現行の記録と混ざりません。

Windows の退路は、まだ 1.28.3 では作っていません（NOT VERIFIED）。
次に `native/target/sherpa-gpu-ort1282` が在りますが、それは ORT 1.28.2 の build で、RP-24 が WSL の 1.28.3 と比べれば ORT の版の不一致で落ちます。

### Windows の ORT 1.28.2 の build

ORT 1.28.2 の build は `native/target/sherpa-gpu-ort1282` に残してあり、消していません。`build.json` と `rp17_passed.json` もそこに在ります（別の dir なので、現行の記録とは混ざりません）。
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

WSL では、`sherpa-onnx-sys` が `libonnxruntime.so`、`libonnxruntime.so.1`、`libonnxruntime.so.1.31.1` を別々の file として複写します（symlink ではなく複写）。
そこへ `libonnxruntime.so` を完全な path で先読みすると、`libsherpa-onnx-c-api.so` が名前（SONAME）で要求する `libonnxruntime.so.1` が別の file として読み込まれ、ORT が 2 つ入りました（`onnxruntime_libraries` が 2。RP-17 が捕まえる形）。
そこで Linux では先読みをせず、`libsherpa-onnx-c-api.so` の RUNPATH（`$ORIGIN`）に任せて、版の問い合わせも SONAME の `libonnxruntime.so.1` で行います。
Linux には `System32` に当たる物が無く、先読みの目的がありません。

## 採らない手段

採ったのは、利用者が指定した「sherpa-onnx の C ライブラリを CUDA 13.4 と ORT の `gpu_cuda13` で自分で build し、`SHERPA_ONNX_LIB_DIR` で繋ぐ」形です。
次の手段は、理由とともに採りません。

- **CUDA 12 版の build を主にすること**：sherpa-onnx の既定は CUDA 12 版の ORT ですが、CUDA 13.4 の環境があるので使いません。CUDA 13 版が動かないときの退避先としてだけ残してあり、手順は設計の 3.12 にあります。
- **ORT を退路の 1.28 系のまま測定へ進むこと**：`ort` crate の既定の API 版が 1.31 で、1.28 系では api-28 を指定する外れ道を通ることになり、測定を後で測り直す羽目になるため、1.31.0 に上げました。退路の 1.28.3 の build は残してあります。`hayamimi-gpu` 自身は `ort` crate に依存していません。
- **公開済みの CUDA 版を取ること**：`sherpa-onnx-sys` の build script は CUDA 版を自分では取りません。版と option を固定して再現するために、自分で build します。
- **cloud の認識 API と実行時の Python**：速度を最優先にするため使いません。
- **provider の library の文字列で CUDA の版を判別すること**：両方の主版の文字列が在るため、判別できません。
- **`native/hayamimi-eval` の変更**：この build と接続は触りません。

## 戻し方

作った物は、`--out-dir` の下にだけ在ります（既定は `native\target\sherpa-gpu`、WSL は作業 directory）。
消すと build 前に戻り、他の物は変わりません。

```powershell
Remove-Item -Recurse -Force "<repo>\native\target\sherpa-gpu"
```

Windows の現行（v2.0.1 と ORT 1.31.1。WSL の現行も同じ版の `~/.local/share/hayamimi/build/cuda1343-sherpa201`）の build は `native\target\sherpa-gpu` に在ります。2 回目の build は `native\target\sherpa-gpu-repeat` に、退路（ORT 1.28.3）の build は `native\target\sherpa-gpu-ort1283` に作る置き場で、2026-10-29 の時点では Windows のこの木に無く（Windows の 1.28.3 は未作成、WSL は symlink）、作った物はどれも同じ方法で消せます。
Windows の `native\target\sherpa-gpu-ort1282` は ORT 1.28.2 の build で、退路ではありません。
退路に戻すには、`SHERPA_ONNX_LIB_DIR` を `native\target\sherpa-gpu-ort1283\<os>\install\lib` にして build します（build.json の ORT の版が 1.28.3 になるので、RP-17 はその版で判定します）。
WSL に入れた CUDA と cuDNN の戻し方は、「手順（WSL）」の「0. 入れる前の戻し方」にあります。

Rust 側を CPU 版に戻すには、`SHERPA_ONNX_LIB_DIR` を設定せずに build します。
build script が CPU 版の archive を取ります（設計 3.12）。
CUDA 12 版へ戻す手順は、設計の 3.12 に書いてあります。

## 段の計測に使う llama-server（WSL の user unit）

段の計測（`hayamimi-eval stages`）の「訳 T」の行と、負荷の下の計測は、llama-server を使います。
走っている llama-server が無いときは、試し用の unit を起こします。
WSL の hook が llama-server の素起動を止めるため、unit 経由で起こします。
走っているかは `ss -tlnp 'sport = :18091'` で見ます（待ち受けが在れば port から pid が引けます）。

| unit | port | 層 | 置き場 |
|---|---|---|---|
| `hayamimi-llama-trial-gpu.service` | 18091 | `-ngl 99`（GPU。負荷の相手と、訳 T の GPU の行） | `~/.config/systemd/user/` |
| `hayamimi-llama-trial-cpu.service` | 18092 | `-ngl 0`（CPU。訳 T の CPU の行） | 同上 |

model は `/mnt/h3-models/ollama/CAT-Translate-3.3b-Q8_0.gguf`、binary は `~/source/llama.cpp/build-cuda120/bin/llama-server`、model と binary は、訳の試し（`testdata/probe1_*.log`）と同じです。
CPU の unit は `-c 4096 -np 2`、GPU の unit は `-c 8192 -np 4` です。
GPU の unit は、負荷の client 2 本と計る依頼 1 本が、別々の slot を使えるように slot を 4 つにしています。
この llama-server は WSL の CUDA 12.0 の build で、本番の形（Windows の native、CUDA 13 版）とは違います。

```bash
systemctl --user start hayamimi-llama-trial-gpu.service   # 起こす
systemctl --user stop hayamimi-llama-trial-gpu.service    # 止める（SIGKILL。10 秒以内に止まる）
systemctl --user is-active hayamimi-llama-trial-gpu.service
ss -tlnp 'sport = :18091'                                  # 待ち受けの確認
```

戻し方（unit を消す）：

```bash
systemctl --user stop hayamimi-llama-trial-gpu.service hayamimi-llama-trial-cpu.service
rm ~/.config/systemd/user/hayamimi-llama-trial-gpu.service ~/.config/systemd/user/hayamimi-llama-trial-cpu.service
systemctl --user daemon-reload
```

## 段の計測を走らせる（Windows）

`hayamimi-eval stages` は 1 つの process で 1 つの組（provider、負荷の有無、JIT の cache の状態）を測り、run file（`.jsonl`）を書きます。
組ごとに process を分けるのは、sherpa-onnx が provider を model の生成時に決めるためと、JIT の cache の状態を process の開始時に決めるためです。
前面の process で走らせ、負荷の下の run は、GPU の llama-server（上の 18091）へ訳の依頼を送り続ける client を 2 本、同じ process の中で起こします。

```powershell
# JIT の cache が空の run（空の dir を用意し、その dir を CUDA_CACHE_PATH に指す）
$env:CUDA_CACHE_PATH = "<空の dir>"
hayamimi-eval stages --provider cuda --wav testdata\lat-ja.wav --models models --out <名前>.jsonl --cache empty --run 1 --llama-url http://127.0.0.1:18091
# 同じ dir で続けた温かい run（--run を 2、3 と増やす）
hayamimi-eval stages --provider cuda --wav testdata\lat-ja.wav --models models --out <名前>.jsonl --cache warm --run 2 --llama-url http://127.0.0.1:18091
# 負荷の下（CPU の run は --llama-url を 18092 にし、--load-url は GPU の 18091 のまま）
hayamimi-eval stages --provider cuda --wav testdata\lat-ja.wav --models models --out <名前>.jsonl --run 1 --llama-url http://127.0.0.1:18091 --load-url http://127.0.0.1:18091
```

1 回の転送は、`nsys profile --capture-range=cudaProfilerApi --capture-range-end=repeat:<発話数 × model の数>` の下で `stages --only recognize --profile-ranges --repeat 1`（int8 を CUDA で走らせる対照は `--int8-on-cuda` を足す）を走らせ、認識の 1 呼び出しごとに `nsys stats --format csv` の `cuda_gpu_mem_time_sum`、`cuda_gpu_mem_size_sum` を取り、`hayamimi-eval nsys-transfers --dir <csv の dir> --variant <fp32 か fp16 か int8> --out <名前>.json` で集めます。

句読点の BERT と、認識の encoder だけの時間は、`ort` で model を直に開いて測ります（`--only punct_bert,encoder_only`）。
BERT の入力は `punct_ja_parity.json` の `input_ids` で、`--punct-cases` に渡します。
CUDA で CPU に置かれた node の数は、`hayamimi-eval ort-placed --model <onnx> --provider cuda --out <名前>.json` で数え、`stages-report` の `--placed` に渡します。
設計の 3.5 の表で GPU とした model は全部数えます。句読点の `punct_bert.onnx` と、言語判定の `tiny-encoder.onnx` と `tiny-decoder.onnx` です。
RP-6 は node の表からこの 3 つの行を読み、行が無い GPU の model を落とします。

run file と転送の JSON から、結果の文書を 1 本にします。

```powershell
hayamimi-eval stages-report --runs <run file...> --design docs\design\rust_port.ja.md --build-json native\target\sherpa-gpu\windows\build.json --cpu "<cpu の名前>" --transfers <転送の json...> --python testdata\baseline\lat_ja_0.35_1.jsonl testdata\baseline\lat_ja_0.35_2.jsonl --out docs\results\gpu_stages.md
```

負荷の下の run は、計測の間の CPU と GPU の使用率を 20 秒ごとに 1 行、`native/target/measure/log/load.log` へ追記して確かめました。
計測の間、ほかの重い仕事は並べません。

## 2 音源同時の認識を測る（R7）

2 つ目の音源は、`stages --peers 1` が別 process の `stages-peer` を起こし、run の間ずっと decode させます。
1 つの process に session を並べて開くと abort する既知の問題があるので、thread ではなく process で分けます。
`stages-peer` は標準入力が閉じるまで decode を続け、標準出力に `ready` と `decodes <回数>` を書きます。

CUDA では、`recognize` の後に同じ process で `encoder_only` を開くと、ort が DefaultLogger の error で落ちます。
`recognize` と `encoder_only,search_replay` は process を分けて取ります。
`r7-report` は、provider と peers が同じ run file を 1 本にまとめます。
CPU は分ける必要がありません。

```powershell
# peers は 0 と 1 を provider ごとに取る（CUDA は 2 本に分ける）
hayamimi-eval stages --provider cuda --wav testdata\lat-ja.wav --models models --repeat 5 --peers 1 --only recognize --out <cuda_p1_recognize>.jsonl
hayamimi-eval stages --provider cuda --wav testdata\lat-ja.wav --models models --repeat 5 --peers 1 --only encoder_only,search_replay --out <cuda_p1_search>.jsonl
hayamimi-eval stages --provider cpu --wav testdata\lat-ja.wav --models models --repeat 5 --peers 1 --out <cpu_p1>.jsonl
# 結果の文書
hayamimi-eval r7-report --runs <run file...> --conditions docs\results\r7\conditions.md --out docs\results\r7_recognition.md
```

`--conditions` の file は手で書いた測定時の条件で、文書の末尾へそのまま写されます。
文書を作り直しても、条件の節は消えません。
元にした run file は `docs/results/r7/` に置いてあります。

GPU の run の前後に `nvidia-smi --query-compute-apps=pid,used_memory --format=csv` を打ち、ほかの process の GPU 利用を記録します。
`stages-report` は peers が 0 でない run を受け付けません。2 音源の run は `r7-report` だけが読みます。

## 未検証

- 認識結果の文字数が 44 で、golden の期待値（45 文字）と 1 文字違います。文字そのものは比べていません。
- System32 の DLL に結びつく条件。上の遅延読み込みと事前読み込みは、probe の実行で防げたことだけを確かめています。
- 1 回目の `decode_ms` が遅い理由（初期化か、PTX の変換か）と、`sm_120a` の機械語が実際に使われているか。
- 認識を test の process の中で走らせるときの「実行物の隣」の解き方（上記）。
- 退路（ORT 1.28.3）の build を Windows で作ること。WSL の退路は作って RP-17 を通しましたが、Windows の 1.28.3 は作っていません。
- JIT の cache が空の run は、Windows で 1 回だけです。空の run と温かい run の 1 回目の `decode_ms`（529 ms と、358 から 381 ms）の差には、PTX の JIT と、file が冷たいことの両方が入っていて、分けていません。
- 段の計測の WSL 側は、CUDA の無負荷を 2 回だけ測りました。認識の p50 は 175 ms と 144 ms で、RP-23 の 10% に収まらず、結果の文書には入れていません。
- 段の計測の llama-server は WSL の CUDA 12.0 の build で、本番の形（Windows の native、CUDA 13）ではありません。
- CUDA 13.4.3 での 1 回目の `decode_ms`。4 回の値は Windows が 658 から 731 ms、WSL が 569 から 621 ms で、13.4.1 の 1 回目の遅れ（2469 ms、952 ms）は出ていません。ただし 13.4.3 の run は build の直後ではなく、GPU が温まった後なので、遅れが消えたとは言えません。
