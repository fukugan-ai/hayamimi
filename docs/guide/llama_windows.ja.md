# 翻訳 server（llama-server）を Windows で起こす

`hayamimi llama start --model <gguf の名前>` は、llama.cpp の `llama-server` を Windows の独立した process として起こします。
端末を閉じても、`wsl --shutdown` を打っても、server は生きて `http://127.0.0.1:8099` で答え続けます。
GPU が無い機械、または GPU で立ち上がらない機械では、同じ model を CPU で動かし、そのことを 1 回だけ言います。
どちらでも止まりません。

```text
hayamimi llama start
  │
  ├─ CUDA の device を数える ── 在る ─> CUDA build、全層を GPU に置く（-ngl 99）
  │                          └─ 無い ─> CUDA build、層は 0（-ngl 0）。gpu.device.unavailable を出す
  │
  ├─ 立ち上がらなければ ─────────────> CPU build（-ngl 0）
  │
  └─ /health が ok を返したら終わる ─> CPU に落ちたときだけ gpu.fallback.cpu（stage = translate）を出す
```

図の結論：起こす手は最大 2 つで、2 つとも立ち上がらないときだけ終了 code 1 になります。

## 置き場

すべて `%LOCALAPPDATA%\hayamimi` の下です。
`--dir <root>` で別の root に替えられます。

| 物 | path |
|---|---|
| CUDA build | `llama\cuda\llama-server.exe`（隣に DLL） |
| CPU build | `llama\cpu\llama-server.exe`（隣に DLL） |
| 入れた release の記録 | `llama\manifest.json` |
| model | `models\llm\<名前>.gguf` |
| server の出力 | `log\llama-server.log`（前回の分は `llama-server.prev.log`） |

## 起こす

前提は、build と model が入っていることです（入れ方は次の節）。

```powershell
hayamimi llama start --model CAT-Translate-3.3b-Q8_0.gguf
```

| option | 意味 | 既定 |
|---|---|---|
| `--model` | `models\llm` の file 名、または path。必須 | なし |
| `--port` | server の port | 8099 |
| `--wait` | 答えるまで待つ秒数 | 180 |
| `--dir` | install の root | `%LOCALAPPDATA%\hayamimi` |

server の引数は `-m <model> --host 127.0.0.1 --port <port> -np 4 -c 8192 -ngl <層>` です。
`-np 4` は同時の依頼 4 本（マイクの確定行、ループバックの確定行、途中の行、直し）の分です。
同じ port で server がすでに答えているときは、何も起こさず `translate.server.running` を出して終了 code 0 になります。

確かめます。

```powershell
(Invoke-RestMethod http://127.0.0.1:8099/health).status
```

出力は `ok` です。

終了 code は 0（server が答えた）、1（答える server が残らなかった、または model が無い）、2（command line の誤り）です。

## 入れる

llama.cpp の release と model を `%LOCALAPPDATA%\hayamimi` の下に置きます。
次の手順は WSL から打ちます。
`DATA` は Windows 側の path を WSL から見た形です（`<user>` は Windows のユーザー名）。

```bash
DATA=/mnt/c/Users/<user>/AppData/Local/hayamimi
TAG=$(curl -s 'https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=1' | jq -r '.[0].tag_name')
B=https://github.com/ggml-org/llama.cpp/releases/download/$TAG
mkdir -p $DATA/llama/cuda $DATA/llama/cpu $DATA/models/llm $DATA/log
```

asset の名前は release ごとに変わります。
次で一覧を出し、`win-cuda` と `win-cpu` の x64 の zip、CUDA 版に添える `cudart` の zip を選びます。

```bash
curl -s "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/$TAG" | jq -r '.assets[].name' | grep -E 'win-(cuda|cpu)|cudart'
```

選んだ zip を取り、展開します。
CUDA 版は、build の zip と `cudart` の zip を同じ directory に展開します。

```bash
curl -sSL -o cuda.zip $B/<CUDA build の zip>
curl -sSL -o cudart.zip $B/<cudart の zip>
curl -sSL -o cpu.zip $B/<CPU build の zip>
unzip -q -o cuda.zip -d $DATA/llama/cuda
unzip -q -o cudart.zip -d $DATA/llama/cuda
unzip -q -o cpu.zip -d $DATA/llama/cpu
```

model は隣に書いてから rename します。

```bash
cp <元の gguf> $DATA/models/llm/<名前>.gguf.part && mv $DATA/models/llm/<名前>.gguf.part $DATA/models/llm/<名前>.gguf
```

取った zip の `sha256sum` と、`TAG`、取得した時刻を `llama\manifest.json` に書き残します（`start` はこの file の `tag` を `translate.server.started` の `tag` に写すだけで、決定には使いません）。
`tag` は文字列、`builds.<cuda|cpu>.assets[]` は `name`、`bytes`、`sha256` です。

## 入れた release の記録

| 項目 | 値 |
|---|---|
| tag | b11609 |
| 公開 | 2026-10-14T00:51:19Z |
| 取得 | 2026-10-14T13:32:00+09:00 |
| CUDA build | `llama-b11609-bin-win-cuda-13.4-x64.zip`（153484683 bytes、sha256 `c6aa29c9460e2b01c628cd5ac19a9d884fd491ad411df62b24444fde2ac4e434`） |
| CUDA runtime | `cudart-llama-bin-win-cuda-13.4-x64.zip`（423535356 bytes、sha256 `738f8c251ac22b70c3ae6f83a10cf222725df0395246a2cf58f32bdb85fbe668`） |
| CPU build | `llama-b11609-bin-win-cpu-x64.zip`（19518618 bytes、sha256 `8708a7b81842333927f6377ac3f17ff8bba4dbe1ee0cf28dfd5e7a32e3a46b38`） |

b11609 は 2026-10-14 に `/releases?per_page=1` が返した最新です。
llama.cpp の b 番号の release は prerelease の印が付くので、`/releases/latest` は最新を指しません。
上げるときは、この表を新しい tag の値に替え、`hayamimi llama start` を CPU と GPU で打ち直します。

## 実機で測った結果

2026-10-14、Windows 側の実機（GPU 在り）で打ちました。
出力は `docs/results/r8b/` にあります。

| 場面 | 結果 | 記録 |
|---|---|---|
| GPU を見せて起こす | `build` = `cuda`、`gpu_layers` = 99、終了 code 0 | `start_cuda.log` |
| GPU を隠して起こす（`CUDA_VISIBLE_DEVICES` = `-1`） | `gpu.device.unavailable`（`no_device`）と `gpu.fallback.cpu`（`stage` = `translate`）を 1 件ずつ、`gpu_layers` = 0、終了 code 0 | `start_gpu_hidden.log` |
| CUDA build を置かずに起こす | `translate.server.failed`（`build` = `cuda`、`reason` = `missing_build`）、`gpu.fallback.cpu`（`session_failed`）、`build` = `cpu` で答える、終了 code 0 | `start_no_cuda_build.log` |

起こした command は、server を子に持たず、端末の出力を握らせずに終わります。
Windows は子に親の継承可能な handle を引き継ぐので、起こす前に自分の handle の継承を外しています（`native/hayamimi/src/llama/handles.rs`）。
外さないと、起こした command を呼んだ側が server の生きる間 待たされます。
CPU で動かした server の log には `Flash Attention not supported, set to disabled` が出ます。

## 止める、戻す

stop の command はありません（未実装）。
port を持つ process を探して止めます。

```powershell
Get-NetTCPConnection -LocalPort 8099 -State Listen | Select-Object -ExpandProperty OwningProcess
Stop-Process -Id <出た id>
```

確かめます。止めた後は `Invoke-RestMethod` が接続できないと言って失敗します。

```powershell
Invoke-RestMethod http://127.0.0.1:8099/health
```

入れた物を戻すには、`%LOCALAPPDATA%\hayamimi\llama` と `models\llm` と `log` を消します。
registry、予定表、service には何も書いていません。

## 未検証

- CS-20 の `wsl --shutdown` の後の応答は、まだ打っていません。打つと、その時に動いている WSL の session も止まります。端末をすべて閉じ、`wsl --shutdown` を打って 10 分待った後に、Windows 側で次を打ちます。`ok` が返れば通りです。

```powershell
cargo test --release -p hayamimi --test cs20 -- --ignored
```

- 字幕の server が訳の段の `/events` に `fallback` を送る経路は、この command にはありません。log と stderr には出ます。
- GPU が在る機械での `session_failed`（CUDA の起動が失敗し CPU build に落ちる）は、CUDA build を置かない場面で確かめました。CUDA build が起動後に落ちる場面は打っていません。
