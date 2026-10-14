# R8e の実測（2026-10-14）

RP-3 の Rust の run と、それを読む RP-5、RP-22、RP-25 の結果。
他の席が同じ GPU と CPU を使っている最中の値で、無負荷の値ではない。

## 構成

| 物 | 内容 |
|---|---|
| 認識 | `hayamimi --input ws --ws-port 8766 --serve 8833 --min-silence 0.30` |
| 訳 | `hayamimi-relay live --events http://127.0.0.1:8833/events --llm http://127.0.0.1:8080 --serve 8835` |
| 訳の server | WSL の llama.cpp b11628（`-ngl 99 -c 16384`、slot 4）、CAT-Translate-3.3b-Q8_0 |
| 流す | `hayamimi-eval stream --wav lat-ja.wav --repeat 5 --port 8766 --relay-port 8835 --out rp3_rust.jsonl` |

run の file は `rp3_rust.jsonl`（標本）、`rp3_rust.events.jsonl`（受けた event）、`rp3_rust.summary.json`（要約）。

## L（発話の終わりから訳の字幕）

| | n | p50 | p95 |
|---|---|---|---|
| Rust `L` | 93 | 1710 ms | 3145 ms |
| Rust `L_asr` | 93 | 1270 ms | 2044 ms |
| Rust `L_tr` | 93 | 439 ms | 1526 ms |
| Python `L`（`--min-silence` 0.30、1 回） | 95 | 1481 ms | 2008 ms |
| Python `L_asr` | 95 | 1104 ms | 1724 ms |
| Python `L_tr` | 95 | 212 ms | 1019 ms |

同じ構成を、`cargo test` の実走を重ねて負荷を上げた間に流し直すと、`L` の p50 は 2700 ms、p95 は 3384 ms だった（n = 97）。
Python 以下という RP-3 の合否は、どちらの run でも満たしていない。
値は `stream` の要約で、`rp03` の判定の test はまだ無い。

## RP-5（速報）

`rust_partials_arrive_no_later_than_python` は落ちる。
比べた Python の基準は `rp05.rs` が読む `--min-silence 0.35` の run（`lat_ja_0.35_1.jsonl`、`final` を持つ発話 103 件）で、Rust の run は 0.30 である。
最初の `partial` の遅れの p95 は Rust 4398 ms（`jq` で数えた nearest-rank では 4397 ms）、Python 3776 ms（同 3775 ms）。
`--min-silence 0.30` の Python の run（`lat_ja_0.30_1.jsonl`、101 件）の p95 は 3751 ms で、どちらの基準でも Rust は負ける。
最初の `partial` の遅れは `--min-silence` に依らず、0.35 を基準にしても結論は変わらない。
p50 は 1912 ms 対 1870 ms でほぼ同じで、遅い側の 5 件（発話 17 の 5 回）は両方に在る（Rust 4397〜4445 ms、Python 4471〜4573 ms）。
p95 の位置が分かれるのは母数が違うからである。
`final` を持つ発話が Rust 93、Python 103 で、Rust は発話 11、12、13、18、22 が `final` を持たない回が在る（Python は 10、18、19）。
流し直した run では p95 が 3921 ms（n = 97）で、母数が変わると値が動く。
差の原因は速度よりも、`final` を出さない回の多さである。

## RP-22（GPU の memory）

`nvidia-smi --query-gpu=memory.used` を 1 秒ごとに読んだ 873 個（`rp22_vram.txt`）の最大は 11981 MiB で、上限 14000 MiB 以下。
起動前の 3 回は 9803 MiB（`rp22_vram_before.txt`）。
この値には他の席の process と llama-server が入っている。

## RP-25（訳の揺れ）

`rp25_translation_flicker_is_bounded` が通る。
表示した行 192、先の訳から確定の訳へ置き換わった行 18、速報の訳 390 件、確定した接頭辞が消えた回数 0。
外の送り先（Zoom、Teams、SRT）は R8g で、この run には `Sent` が無い。

## H6 と `cache_prompt`

`native/hayamimi-relay/tests/r8e_server.rs` の 3 本（`--ignored`、port 8080 の server が要る）。

| 確かめた物 | 値 |
|---|---|
| 閉じない stream | 最初の token の 300 ms 後に slot 1 本が処理中、`[DONE]` の後に空く |
| 最初の token の後に接続を閉じる | slot が空くまで 44 ms（上限 1 s） |
| 同じ依頼の 2 回目 | `timings.cache_n` = 31 |

## H39（道具の版）

| 物 | 結果 |
|---|---|
| Windows の置き場（`%LOCALAPPDATA%\hayamimi\llama`） | 入っているのは b11609。GitHub の release の一覧（`/releases?per_page=1`）の先頭も b11609 で、3 つの zip の size と sha256 が release の digest と一致した。入れ替える物が無い |
| WSL の llama.cpp | b11577 から b11628（`a1361c790`）へ上げた。`cmake --build build-cuda120 --config Release -j 4` |
| 前の build | tar に控えた（`build-cuda120` と HEAD `ec483d3f3`）。戻し方は `docs/guide/run_hayamimi.ja.md` の「WSL の llama.cpp を戻す」 |
| 打ち直し | RP-16 の訳の行（`rp16` の translate 14 本と `visible_gpu` の 2 本）と CS-20 の 001〜004 が通る。CS-20 の 005 は `wsl --shutdown` を伴うので打っていない |

## RP-29 の port の場面

`missing_resources_fall_back_in_real_runs` と `blocked_ingest_port_moves_in_a_real_run` を、他の席が動いている machine で 3 回、`stream` の run を重ねた machine で 3 回打った。
`blocked_ingest_port_moves_in_a_real_run` は 6 回とも通った。
`missing_resources_fall_back_in_real_runs` は 5 回通り、負荷を重ねた 3 回目だけ `port: no final event came out` で落ちた（その回は 1390 秒、他は約 240 秒）。

- `hayamimi` は `/events` の server を、model を開く前に立てる（`run.rs` の `execute` で `start_servers` が `open_engine` より前）。client の待ちは model を開く数秒で、20 ms 間隔の接続より十分長い。負荷は待ちを長くする向きに働く。
- 遅れて繋いだ client が先の `final` を受けないのは、live の購読に replay が無いためで、設計どおりである。ただし client が model を開く前に繋ぐので、今の順では起こらない。
- 落ちた 1 回の原因は特定していない（NOT VERIFIED）。5.8 倍遅かった回に限り、5 回は通っている。疑う物は、候補の port に他の席の listener が居て client がそこへ繋ぐ形と、process 自体の停滞である。確かめるには、落ちた回の client が繋いだ port と受けた event の数を test の error 文に出す。

## 採らない手段

- Windows の llama.cpp の入れ替え。最新の release が入れた物と同じ b11609 で、digest が一致するため。
- RP-5 の閾値を緩める、または母数を揃えるために標本を除く。落ちた値のまま残し、原因を上に書いた。
- RP-29 の client の接続間隔や待ちの上限を変える。待ち不足が原因だという証拠が無いため。
- Python と Rust の `L` を比べる道具の追加。比べる道具は設計の席が RP-3 の側を正と決めており、この slice では `stream` の要約の値を載せるだけにした。
