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
| `--serve [port]` | `/events` を配る。port の既定は 8833 |
| `--serve-host <address>` | `--serve` が待つ address。既定は 127.0.0.1（この機械だけ）。LAN の端末から開くときは `0.0.0.0`（次の節） |
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

## LAN の端末から字幕を見る

`hayamimi` の `--serve` と `hayamimi-relay live` の `--serve` は、既定では 127.0.0.1 だけで待ちます。
`--serve-host` が LAN へ開くのは `--serve` の router（頁、`/events`、`/config`、`/reset` ほか）だけです。
`/config` と `/reset` は認証なしで設定と状態を変えるので、LAN へ開くのは利用者が選んだ時だけにします。
`/ingest`（音声の入力）は `--ws-host` で別に決まり、既定は 127.0.0.1 のままです。`--serve-host` を付けても LAN には開きません。

別の端末の browser で頁を開くときは、2 つの process に `--serve-host 0.0.0.0` を付けて起こします。
端から端の受入（E2E）で別の機械の browser から頁を見るときも同じで、process A、process B、中継の command にそれぞれ足します。

```bash
hayamimi --wav lat-ja.wav --models <models の directory> --serve 8833 --serve-host 0.0.0.0
hayamimi-relay live --serve 8835 --serve-host 0.0.0.0
```

loopback でない address で待つと、stderr に LAN の URL が 1 行出ます。
文の後ろに URL が続く形で、`hayamimi` は頁の URL（`http://<LAN の address>:8833/`）、`hayamimi-relay` は `/events` の URL（`http://<LAN の address>:8835/events`）です。
同じ時に、`hayamimi` は log の event `server.lan.opened`（warn、`port` だけを持つ）を stdout に出します。
`hayamimi-relay` は `relay.serve.started` の `scope` が `lan` になります（loopback のときは `loopback`）。
同じ event は、訳を頼む llama-server の address を `llm`（ja と en の組）と `llm_other`（組の外の言語）に持ちます。

URL の address は、待つ address によって決まります。

- `0.0.0.0` で待つと、この機械が外へ出るときの送り元の address（`ip route get` が示す `src`）です。
- 特定の address を渡すと、その address だけで待ち、URL にも同じ address を使います。127.0.0.1 からは繋がりません。
- この機械が持たない address を渡すと、待てません。`hayamimi` は字幕の server の段を外して続けます。stderr の理由は「port が使われています」と出ますが、原因は address です。
- network への経路が無い機械では、address を決められないと 1 行だけ言い、URL は出しません。この機械の address と `--serve` の port で開きます。

受入は RP-40 です。
`hayamimi-relay` の実行物が `hayamimi` の隣（`target/release`）に要るので、先に build します。

```bash
cargo build --release -p hayamimi-relay
cargo test --release -p hayamimi --test rp40 -- --ignored
```

この test は、`hayamimi --wav ja_014.wav --serve 0 --serve-host 0.0.0.0` と `hayamimi-relay live --serve 0 --serve-host 0.0.0.0` を起こします。
stderr の URL の port に、LAN の address と 127.0.0.1 の両方から `GET /` と `GET /events` を打ちます。
`/events` は `text/event-stream` を返し、`hayamimi` の `GET /` は 200 を返します。
中継は `/events` だけを配るので、`GET /` は 404 です。
もう 1 本の test は、`--serve-host` を付けない run で、LAN の address に繋がらず、LAN の URL の行が出ないことを確かめます。

**採らない手段**：

- 既定を `0.0.0.0` にする。認証の無い `/config` と `/reset` が LAN に開くので、利用者が選んだ時だけにします。
- 頁の中身を替える。この slice は待つ address だけを替えます。

## マイクの実機を確かめる（Windows、RDP の録音の転送）

RDP の録音の転送は、Windows に「リモート オーディオ」という録音の端点（音の出入り口）を作ります。
`--input mic` で起こした `hayamimi` がこの端点を開き、装置の名前を log に出すところまでは、機械で確かめられます。
人の声が字幕になることは、話す者が居る時にしか確かめられません。

端点は RDP の session の中にだけ在ります。
session を切る、ログオフする、既定の録音の装置を切り替える、のどれも打ちません。
端点が消えるか、使う側の音声の設定が変わるためです。

WSL の中から Windows の command を打ちます。
`powershell.exe` は PATH に無いので、絶対 path で呼びます。

```bash
PS=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe
/mnt/c/Windows/System32/qwinsta.exe
$PS -NoProfile -Command 'Get-PnpDevice -Class AudioEndpoint | ForEach-Object { $_.InstanceId }'
$PS -NoProfile -Command '"session=" + (Get-Process -Id $PID).SessionId'
```

1 本目は RDP の session が `Active` かを、2 本目は音声の端点の一覧を、3 本目は WSL から起こした Windows の process の session の ID を見ます。
3 本目の値が `qwinsta.exe` で `>` が付いた行の ID と同じなら、起こした process から端点が見えます。

実行物は Windows の cargo で build します。
並列度は `C:\Users\<user>\.cargo\config.toml` の 2 のままで、`--jobs 2` を付けても同じです。
worktree には GPU 版の sherpa-onnx の build が無いので、`SHERPA_ONNX_LIB_DIR` には本体の checkout の build を絶対 path で渡します。

```bash
$PS -NoProfile -Command 'Set-Location <repo の Windows path>\native; $env:SHERPA_ONNX_LIB_DIR = "<本体の checkout の Windows path>\native\target\sherpa-gpu\windows\install\lib"; & C:\Users\<user>\.cargo\bin\cargo.exe build --release --jobs 2 --target x86_64-pc-windows-msvc -p hayamimi'
```

process A（マイクの音を ja で認識する）を起こします。
`<log dir>` は repo の外の作業 directory です。
log は stdout の JSONL で、stderr は空です。

```bash
cd <repo>/native/target/x86_64-pc-windows-msvc/release
setsid nohup ./hayamimi.exe --input mic --lang ja --models '<Windows 形式の models の path>' --serve 8833 --ws-port 8766 \
  > <log dir>/hayamimi.stdout.log 2> <log dir>/hayamimi.stderr.log < /dev/null &
$PS -NoProfile -Command 'Get-Process hayamimi | Select-Object Id,StartTime | Format-Table -AutoSize | Out-String'
```

最後の行で、起こした Windows の pid を控えます。
`--mic-device` を付けなければ、Windows の既定の録音の装置を開きます。
録音の端点がこの 1 つだけなら、既定の装置もこれです。

RDP の画面の browser で `http://127.0.0.1:8833/` を開くと、認識の字幕が見えます。
頁の殻は実行物に入っていますが、`ui/`（wasm と js）は実行物の隣に置く必要があります。
Windows に wasm の target と `wasm-bindgen` が無いときは、在る側（WSL）で上の「画面（Dioxus の 3 頁）を配る」の 2 つの command を打ち、出力を Windows の実行物の隣へ置きます。
`wasm-bindgen` の版は `Cargo.lock` の `wasm-bindgen` と同じにします。

```bash
wasm-bindgen --target web --no-typescript --out-dir <repo>/native/target/x86_64-pc-windows-msvc/release/ui --out-name hayamimi-ui \
  <wasm の target dir>/wasm32-unknown-unknown/release/hayamimi-ui.wasm
```

process B（出力の装置の loopback を en で認識する）は、port を替えて同じ形で起こします。
loopback は再生の端点（RDP では「リモート オーディオ」の再生側）を通る音を拾い、`capture.device.opened` の `kind` が `speaker` になります。
頁は `http://127.0.0.1:8834/` です。

```bash
setsid nohup ./hayamimi.exe --input speaker --lang en --models '<Windows 形式の models の path>' --serve 8834 --ws-port 8767 \
  > <log dir>/procB.stdout.log 2> <log dir>/procB.stderr.log < /dev/null &
```

訳も出すときは、`hayamimi-relay` を Windows の cargo で build して起こします。
2 つの process の訳を出すときは、引数なしの `hayamimi-relay live` が 8833 と 8834 の両方を読みます。
訳の server は WSL の user unit `llama-artisan@CAT-Translate-3.3b-Q8_0.service`（port 8080）で、Windows の `Invoke-WebRequest http://127.0.0.1:8080/health` が 200 を返せば届いています。
訳は `http://127.0.0.1:8835/events` に出ます。
`hayamimi` の頁が読むのは同じ origin の `/events` なので、8833 の頁には訳は出ません。

```bash
setsid nohup ./hayamimi-relay.exe live --events http://127.0.0.1:8833/events --llm http://127.0.0.1:8080 --serve 8835 \
  > <log dir>/relay.stdout.log 2> <log dir>/relay.stderr.log < /dev/null &
```

0 件の判定は、同じ command の中で、必ず出る event も数えます。

```bash
for e in capture.fallback.none capture.device.opened asr.model.loaded asr.utterance.timed; do
  printf '%s ' $e; grep -c "\"event\":\"$e\"" <log dir>/hayamimi.stdout.log
done
grep '"event":"capture.device.opened"' <log dir>/hayamimi.stdout.log
```

| event | 期待値 | 読み方 |
|---|---|---|
| `capture.fallback.none` | 0 | 開ける録音の装置が在った |
| `capture.device.opened` | 1 | `device` が装置の名前（「リモート オーディオ」）、`kind` が `mic` |
| `asr.model.loaded` | 1 以上 | 起動が済んでいる。0 件の対照になる |
| `asr.utterance.timed` | 音が来ている間は増える | 録音の転送から音が届いている |

`capture.fallback.none` を数える語が正しいことは、存在しない装置を名指しした run で確かめます。
`--serve 0` で、process A の port とぶつかりません。

```bash
setsid nohup ./hayamimi.exe --input mic --mic-device zzz-no-such-device --lang ja --models '<Windows 形式の models の path>' --serve 0 \
  > <log dir>/negctl.stdout.log 2> <log dir>/negctl.stderr.log < /dev/null &
grep -c '"event":"capture.fallback.none"' <log dir>/negctl.stdout.log
```

出力は 1 になります。

止めるのは、自分が起こした process の pid だけです。
`pgrep -f` は自分の shell にも当たるので使いません。

```bash
$PS -NoProfile -Command 'Stop-Process -Id <pid>'
$PS -NoProfile -Command '"remaining: " + @(Get-Process hayamimi -ErrorAction SilentlyContinue).Count'
```

**採らない手段**：

- 既定の録音の装置を切り替えて端点を選ぶ。使う側の音声の設定が変わります。
- RDP の session を切って、端点を作り直す。端点は繋いでいる session の中にだけ在ります。

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

## 確定の前の待ちと GPU の負荷を残す

`asr.feed.waited` は、`final` を作った window が queue に置かれてから loop に取られるまでの待ちを、`final` ごとに 1 行出します（`id`、`wait_ms`、`backlog`）。
RP-34 の `L_asr − decode_ms` のうち、loop が前の window の処理で塞がっていた分を `wait_ms` で読みます。

```bash
jq -s -r '[.[]|select(.event=="asr.feed.waited")|.wait_ms]|sort as $v|($v|length) as $n|"n=\($n) p50=\($v[(($n*0.5)|ceil)-1]) p95=\($v[(($n*0.95)|ceil)-1])"' <hayamimi の stdout を残した file>
```

`hayamimi-eval stream --out <run>.jsonl` は `<run>.gpu.json` も書きます。
run の前と後に `nvidia-smi --query-gpu=utilization.gpu,memory.used --format=csv,noheader,nounits` を 1 回ずつ読んだ値です。
読めなかった側は `null` で、run は止めずに warn の `eval.stream.gpu_unreadable` を出します。

Python と Rust の対は、run の前の読みのどちらかの `utilization.gpu` が 5 % を超えるか、`memory.used` の差が 256 MiB を超えれば無効です（`hayamimi_eval::gpu_load::pair_is_valid`。2 つの幅は仮の値）。

訳の取れた割合は `hayamimi_eval::compare::check_translated_share` が数えます。
分母は RP-3 と同じ有効の判定（`is_valid_l_asr`）に入る標本で、`held` だけの標本も入り、`t_tr` が取れなければ分子に入りません。
20 / 23 に届かない run は `L` が小さくても落ちます。

`<run>.summary.json` の `late_final_suspects` は、`final` が `t1_s` を持たない Python の run でだけ数を持ちます。
前の窓が `final` なしで、次の窓に `final` が 2 件以上在る回の数です。
Rust の run は `null` で、数えていないことを表します。

## 早い終わりの保留と、訳の最初の依頼を log で読む

`asr.endpoint.held`（info）は、早い終わりの候補が文を読んだのに確定せず、無音を待った回に 1 行出ます。
項目は `reason`、`span_ms`、`min_ms`、`tail`、`shadow_complete` です。
`tail` は持った文の末尾の種類（`noun`、`honorific`、`particle`、`interjection`、`other`）で、文そのものは載りません。
`shadow_complete` は、名詞止めと敬称を完結として数える規則なら、その場で確定していたかどうかです。確定の動きは変わりません。
確定の行 `asr.endpoint.decided` の `plan` は、持った文を使い回した `held`、segment 全体を読み直した `decode`、線より後ろだけを読み直した `rest` です。`held_gap_ms` は、持った文があったのに読み直した回の、2 つの終わりの差（ms）です。
`reason` が `incomplete` なら文が完結していない候補で、`too_short` なら完結していても声が `min_ms` に満たない候補です。
0 件の判定は、同じ command の中で、必ず出る event も数えます。

```bash
for r in incomplete too_short; do
  printf '%s ' $r; grep '"event":"asr.endpoint.held"' <hayamimi の stdout を残した file> | grep -c "\"reason\":\"$r\""
done
grep -c '"event":"asr.endpoint.decided"' <hayamimi の stdout を残した file>
```

`asr.endpoint.decided` は確定文ごとに出るので、保留の数の対照になります。

`hayamimi-relay live` は、`relay.serve.started` を出す前に、訳の server（`--llm`）へ短い依頼を 2 本、T の形、E の形の順に 1 本ずつ送ります。
依頼ごとに、返れば `translate.warmup.sent`（info。`form`、`dependency`、`duration_ms`）、返らなければ `translate.warmup.failed`（warn。`form`、`dependency`、`duration_ms`、`detail`、`error_code`）が 1 行出て、起動は続きます。
`form` は T の形が `t`、E の形が `e` です。
2 本の待ちは合わせて最長 10 秒で、T の形に 10 秒を使い切ると E の形は送らず、`translate.warmup.failed` の `detail` が `budget` になります。
`detail` は失敗の種類（`transport`、`status 503` の形）だけで、address は載りません。

```bash
grep -E '"event":"(translate.warmup.sent|translate.warmup.failed|relay.serve.started)"' <relay の stdout を残した file>
```

出力の順は、T の形の warm-up の event、E の形の warm-up の event、`relay.serve.started` です。
訳の server をまだ起こしていない run では、2 本とも `translate.warmup.failed` が出ます。`detail` は `transport` です。

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

出力の 1 行の形は R11 より前の Python の採点（`refine_diarize` の method）と同じで、`--out` の file にも同じ文字が入ります。
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
| RP-40（LAN から届く。先に `cargo build --release -p hayamimi-relay`） | `cargo test --release -p hayamimi --test rp40 -- --ignored` |
| RP-9（実行時の Python が 0 本。先に `cargo build --release -p hayamimi-relay`。下の「RP-9 の実走」を読む） | `cargo test --release -p hayamimi --test rp09 -- --ignored --nocapture` |
| RP-21（`models fetch`。実際に取得するので network を使う） | `cargo test --release -p hayamimi --test rp21 -- --include-ignored` |
| 文書の link（RP-19） | `cargo run --release -p hayamimi-eval -- doc-links --repo <repo>` |
| RP-2、RP-18（GPU を使う） | `cargo test --release -p hayamimi-asr --test rp02 --test rp18 -- --include-ignored` |

GPU を隠す環境変数は、Linux では `CUDA_VISIBLE_DEVICES` を空にします。
Windows の driver は空の値を無視するので、`-1` にします。
Windows は言語を system の設定から取り、`LANG` では変わりません。
言語の比較（en と ja）は Linux でだけ打ちます。

## RP-9 の実走（実行時の Python が 0 本）

`rp09_no_python_at_run_time` は、`hayamimi` と `hayamimi-relay` を起こし、子孫の process の名前を 1 秒ごとに集めます。
Windows では `hayamimi --input mix --serve` と `hayamimi-relay live` を 10 分動かします。
WSL では `hayamimi` が `ja_014.wav` を最後まで流す間だけ見ます（中継は起こしません）。
集めた名前に `python` を含む物が 1 つでも在れば落ちます。
標準エラーに、読んだ回数、見た process の数、重複を除いた名前の数と名前が 1 行で出るので、`--nocapture` を付けます。

Windows で打つときは、`native\target\sherpa-gpu\windows\install\lib` を PATH の先頭に足します。
`hayamimi.exe` は `sherpa-onnx-c-api.dll` を実行時に読むためです。
2 つの process が待つ port は既定（`hayamimi` が 8833、`hayamimi-relay` が 8835）なので、打つ前に空いていることと、`hayamimi.exe` が 0 本であることを確かめます。
process の一覧は `Get-CimInstance Win32_Process` を 1 回ずつ読むので、読む間隔は 1 秒より長くなります。

DLL の名前は、`dumpbin` を手で打って見ます。

```bash
dumpbin /dependents <repo の Windows path>\native\target\x86_64-pc-windows-msvc\release\hayamimi.exe
```

出力の `.dll` の行に `python` を含む物が 0 であることが条件です。
`sherpa-onnx-c-api.dll` は遅延 load の行に出ます。

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

## 止めた行の理由と訳を読む（R8o）

両方止めた行は訳を出しません。
理由は log の `translate.line.held` で読みます。
`hold_reason` が E の側、`t_hold_reason` が T の側の理由です。
T が V を通った行には `t_hold_reason` が付きません。
log には訳の本文を載せません。

本文を読む時だけ、`live` に repo の外の file を `--verify-text` で渡します。

```bash
hayamimi-relay live --events http://127.0.0.1:8833/events --llm http://127.0.0.1:8080 --serve 8835 --verify-text <repo の外の dir>/verify-text.jsonl
```

V に渡した訳が、T の分と E の分で 1 行ずつ、`{"id","pass","text"}` の JSON で追記されます（`pass` は `t` か `e`）。
`translate.line.held` の `id` と突き合わせて、止めた行の訳を読みます。
`--verify-text` を付けない run は何も書きません。
file は訳の本文を含むので repo に入れません。
file を開けない時は `translate.dump.failed` を 1 件出し、書かずに続けます。

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

確かめます。`/props` の `build_info` が `b11578-ec483d3f3` なら戻っています（`b11578` で始まり、commit が `ec483d3f3`）。
tar の名前の `b11577` は、上げる前に `git describe` が示した最も近い tag で、その checkout の build number は 11578 でした。

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
