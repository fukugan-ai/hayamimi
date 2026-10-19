# Rust と GPU への置き換えの test

`rust_port.ja.md` の受入 ID の下に置く物を、この file に集める。
置くのは 3 つで、R0 の受入（RB-1〜RB-8）、各受入 ID の下の case 行、RP-19 の対応表である。
受入 ID と test の名前と通る条件はこの file と `rust_port.ja.md` の 6 と 7 が持つ。
case 行と対応表の行は test-designer の席が書く。

## 1. R0 の受入

R0 は、置き換える前の Python の速さと誤り率を、同じ機械で測って固定する slice である。
Python の file は 1 行も書かない。
測った値は `testdata/baseline/`（git 管理外）に置き、要約を `docs/results/baseline_python.md`（git 管理下）に書く。
`<py>` は Windows の `.venv\Scripts\python.exe` を指す。
`<ev>` は `native\target\x86_64-pc-windows-msvc\release\hayamimi-eval.exe` を指す。
`<base>` は `testdata/baseline/` を指す。

| ID | 測る物 | command | 期待値 | 陽性対照 | 陰性対照 |
|---|---|---|---|---|---|
| RB-1 | 採点と評価の 5 つが入っている | `<py> -m pip install sacrebleu==2.6.0 jiwer==4.0.0 simpleder==0.0.5 OpenCC==1.4.2 pyarrow==25.0.1`（版は `requirements-dev.txt`）の後に `<py> -m pip show sacrebleu jiwer simpleder OpenCC pyarrow` | 5 つの `Version:` が 2.6.0、4.0.0、0.0.5、1.4.2、25.0.1 | 同じ command で `sherpa-onnx` の `Version:` が 1.13.6 | 入れる前の `.venv` では 5 つとも `not found` |
| RB-2 | 評価の音声と manifest が在る | 1. `<py> scripts/make_realset.py --skip-eval`、`<py> scripts/make_realset_zhko.py --skip-eval`、`<py> scripts/make_realset_yue.py --skip-eval`、`<py> scripts/make_diarset.py` を打ち、`testdata/eval_real*/manifest.json` と `testdata/eval_diar/manifest.json` と wav を作る。2. `<ev> fetch-set --pin <set>/manifest.json --out testdata/manifests/<set>.json` を 4 組で打ち、各行に wav の sha256 と、Python の manifest が持つ出所の項目を足す。3. `<ev> fetch-set --concat` で lat-ja（`tests/golden/ja/` の 8 本と eval_real の ja 15 本）と lat-en（eval_real の en 15 本）を作り、`testdata/manifests/lat-ja.json` と `lat-en.json` に発話の始まりと終わりの sample の位置を書く（始まりは RP-5 が、終わりは RP-3 が読む） | 6 組の manifest が在り、各組の行の数と wav の数が一致。lat-ja が 23 発話、lat-en が 15 発話。Python の manifest に出所の行の番号が無い組は、`baseline_python.md` にその組の名前を書く（R11 の後の取り直しは wav の sha256 で照らす） | 2 回目の `--pin` は同じ file を出す | wav を 1 本消した組で、その wav を名指しして error |
| RB-3 | scorecard を Python で取り直す | `<py> scripts/eval_engine.py` を 2 回。この script は git 管理下の `docs/results/scorecard.md` を上書きする（`eval_engine.py` の `DOCS_PATH`）。各回の前に `docs/results/scorecard.md` を `<base>/scorecard_record.md` へ cp で退避し、各回の後に出力を `<base>/scorecard_run<n>.md` へ cp し、退避した記録を cp で `docs/results/scorecard.md` へ書き戻す。書き戻した後に `git diff --ignore-cr-at-eol --quiet docs/results/scorecard.md` が 0 であることを確かめる（`git restore` は hooks の `guard_commands.rs` が project root で refuse する）。`--ignore-cr-at-eol` を付けるのは、同じ木を Windows の git（`core.autocrlf=true`）と WSL の git（設定なし）が書き、WSL の git では、この option を付けない差分の検査が行の終わりの CRLF と LF の違いだけで 1 を返すからである。比べる相手は `<base>/scorecard_record.md` とする。RB-3 の判定がどの木で打っても同じになるのは、`testdata/baseline/`（git 管理外）を持つ木に限る | 言語ごとの平均の誤り率が記録（ja 0.038、en 0.023、zh 0.066、ko 0.081、yue 0.061）と 0.005 以内。yue の出力に「yueはt2s正規化」が在る。2 回の差を `baseline_python.md` に書く。R0 の終わりに、書き戻した `docs/results/scorecard.md` の中身が退避した記録と byte ごとに同じ（行の終わりを除く）で、`git diff --ignore-cr-at-eol --quiet docs/results/scorecard.md` が 0 | 同じ出力に LID の誤判定の内訳の節が在る | OpenCC の無い `.venv` の run では「opencc無し」が出て、yue の値が記録とずれる |
| RB-4 | golden 8 本を Python で取り直す | `<py> scripts/make_ja_golden.py --check` | 8 本とも `golden.json` の CER と一致し、終了 code が 0 | 同じ command が 8 本の名前を出す | ITN を外した設定で `ja_085` と `ja_090` が外れる |
| RB-5 | Python の遅れ `L` と `L_asr` | Python を `<py> scripts/realtime_transcribe.py --input ws --serve --translate en --min-silence <s>` で起こし、`<ev> stream --wav lat-ja.wav --repeat 5 --out <base>/lat_ja_<s>_<n>.jsonl` を 2 回以上 | 各回で、標本の枠 23 発話 × 5 回 = 115 が 有効 + final なし + 終端不正 に分かれて和が枠と一致する（有効は `final` が在り `L_asr − L_dec ≥ min_silence`）。`L_asr` の p50 と p95 は有効な標本で、`L` と `L_tr` の p50 と p95 は有効な標本のうち訳が取れた物で出る。発話ごとの 5 回の中央値、`L_tr` が負の発話の数、訳の取れない有効な標本の数が出る。回の間の p50 の差を `baseline_python.md` に書く | 同じ run の全 `final` の `latency_ms` の平均と本数を、WS の `final` event と Python の stdout の log の 2 つから別々に出すと、本数が同じで平均の差が 0.5 ms 以内（WS の値は丸めず、log は整数 ms に丸めるので、1 本の誤差は 0.5 ms 以内で、平均の差も 0.5 ms を超えない） | `/ingest` に繋がない run では `t_final` が 0 件で error |
| RB-6 | `--min-silence` の値 | RB-5 を `<s>` = 0.20、0.25、0.30、0.35 で 1 回ずつ。各値で ja の CER も取る | 4 つの値の `L_asr` の p50 と p95 と ja の CER の表が `baseline_python.md` に在り、CER が 0.35 の時を超えない最小の値が 1 つ名指しされている。`L_asr` は各値の run をその run の `min_silence` で仕分けた有効な標本で数える（終端不正と final なしを含めない） | 0.35 の行が RB-5 の 1 回目の run と同じ有効な標本で数えた値で、同じ桁 | 4 行のどれかが欠けた表は通らない |
| RB-7 | 訳の chrF と DER の基準 | 1. `<py> scripts/eval_translate.py --targets <langs> --backend fugumt\|m2m --n 50 --seed 0` で corpus chrF を取る（en は `--backend fugumt`、zh と ko と es は `--backend m2m` で 2 回打ち、出力を `<base>/chrf_eval.log` につなぐ）。2. `<py> scripts/eval_diar.py --collar 0.25 --method refine_diarize` を AMI の 5 会議で 3 回以上打つ（出力を `<base>/der_eval.log` と `<base>/der_eval_run<n>.log`（n = 2、3、…）に置く）。3. chrF の fixture は、`<ev> fleurs-pairs --targets en,zh,ko,es --n 50 --seed 0 --out <base>/pairs/` の参照訳と、同じ参照訳を決まった規則で崩した hyp（`<ev> fleurs-pairs` が `--degrade` で書く。各文の先頭の語を落とし、空白で語を分けない zh は先頭の 1 文字を落とす）の対を、入っている sacrebleu の CLI（`<py> -m sacrebleu <ref> -i <hyp> -m chrf -b -w 4` と、同じ形に `--sentence-level` を足した物。`-w` は小数の桁数で、既定の 1 桁では RP-14 の 0.01 以内を測れない）で採点し、文ごとの値と corpus の値を `<base>/chrf_fixture.tsv` に書く | chrF が en 47.57、zh 20.19、ko 25.20、es 42.09 と 0.01 以内。DER の run が 3 回以上で、どの run も 5 会議の平均を持ち、3 回以上の平均 DER と回の間の幅（最大 − 最小）が `baseline_python.md` に在る（`diarization.md` の 13.9% は入力の音声の sha256 が残っていない記録なので比べない）。`chrf_fixture.tsv` に 4 言語の文ごとの値と corpus の値が在る | 参照訳を hyp にして同じ CLI で採点すると 100。DER は run の会議別の行に `diar_rtf=` が在る（`--method refine_diarize` の run） | 原文を hyp にして採点すると値を割る。`--method` を付けない run（会議別の行に `diar_rtf=` が無い）を DER の run に数えると error |
| RB-8 | wire の録りと要約 | Python を `--input ws --serve` で起こし、`<ev> record --out <base>/wire/` で `/events` と 8 本の route と `/ingest` の handshake を録る。最後に `docs/results/baseline_python.md` を書く | 録れた event の型ごとに 1 件以上の JSON が在り、型の一覧が `baseline_python.md` に在る。`baseline_python.md` に RB-3〜RB-7 の値、測った機械、Python と package の版、日時が在る | `ready` と `final` の型が録れている | server を止めた run では `/events` の接続で error |

`<ev>` の path は target の triple を含むので、`native/` で `cargo build --release --target x86_64-pc-windows-msvc -p hayamimi-eval` と打って作る。
`--target` を付けない build は `native\target\release\` に出て、この path に在る file は古いままになる。

RB-5 の `t_tr` は、Python の `translation` event が `type`、`lang`、`text` だけを持ち、`id` も原文も持たないので、`src_sha256` では取れない。
Python の run では、ある発話の `final` を受けた後、次の発話の `final` を受けるまでに最初に受けた `translation` を、その発話の訳とする。
Python は訳が原文と同じ文字列のとき `translation` を出さない（`realtime_transcribe.py` の翻訳の worker の `out != text` の分岐）。
その間に `translation` が来なかった発話は「訳の取れない発話」として数え、`L` の標本から外し、数を回ごとに `baseline_python.md` に書く。
この規則は Python の run だけに使い、Rust の run は `src_sha256` で対応づける（`rust_port.ja.md` の 3.1）。

RB-6 の ja の CER は、`eval_accuracy.py` が `--min-silence` を引数に持たないので、RB-5 と同じ起こし方で lat-ja の ja の発話を流し、`final` の文字列を参照文と比べて出す（`<ev> stream` の出力から `hayamimi-eval` が計算する）。

## 2. 受入の test の名前

| ID | 受入 test の名前 | 置き場 |
|---|---|---|
| RB-1 | `rb01_scoring_and_eval_packages_are_installed` | `native/hayamimi-eval/tests/rb01.rs` |
| RB-2 | `rb02_eval_sets_match_their_manifests` | `native/hayamimi-eval/tests/rb02.rs` |
| RB-3 | `rb03_python_scorecard_reproduces_record` | `native/hayamimi-eval/tests/rb03.rs` |
| RB-4 | `rb04_python_golden_reproduces_record` | `native/hayamimi-eval/tests/rb04.rs` |
| RB-5 | `rb05_python_latency_has_p50_p95_per_run` | `native/hayamimi-eval/tests/rb05.rs` |
| RB-6 | `rb06_min_silence_sweep_names_one_value` | `native/hayamimi-eval/tests/rb06.rs` |
| RB-7 | `rb07_python_chrf_and_der_reproduce_record` | `native/hayamimi-eval/tests/rb07.rs` |
| RB-8 | `rb08_wire_recording_and_baseline_summary_exist` | `native/hayamimi-eval/tests/rb08.rs` |

RB の test は、R0 で Python を動かした結果の file（`<base>` と `baseline_python.md`）を読んで判定する。
Python を起こすのは test ではなく、R0 の席が上の command を打つ。

## 3. 採らない手段

- **RB の test が Python を子の process に起こす**：Rust の test が Python に依存し、R11 の後に test が壊れる。結果の file を読む形なら、R11 の後も基準の値を読める。
- **Python を 1 回だけ動かす**：回の間の揺れが分からず、Rust との差が揺れの中に入っているかを読めない。
- **文ごとの chrF を出すために Python の script を足すか直す**：R0 は Python の file を書かない。入っている sacrebleu の CLI で同じ値が出る。
- **fixture の hyp を訳の model で作る**：llama-server と訳の評価の道具が R0 の前提に無い。RP-14 は Rust の chrF が sacrebleu と同じ値を出すかを見るので、hyp に訳の質は要らない。
- **scorecard を上書きさせたままにする**：比べる相手の記録が先に書き換わり、記録と比べたことにならない。
- **基準を `testdata/baseline/` だけに置く**：git 管理外で、R11 で Python を消した後に基準が残る保証が無い。要約を git 管理下に置く。

## 4. case 行

各受入 ID（RB-1〜RB-8、RP-1〜RP-70 と RP-72。RP-64 は欠番、RP-71 は 3.18 の段 3 の判定の後）の下の case 行（`T-RB-<n>-nnn`、`T-RP-<n>-nnn`）は、test-designer の席がこの節に書く。
設計の席は書かない。

### 4.1 case 行の読み方

case は `T-<ID>-nnn` で呼ぶ。
受入 test の名前（2 の表と `rust_port.ja.md` の 7）は、その名前の test 関数が 1 本在る。
case 行の test 関数の名前は、各行の「関数の末尾」の前に、何も付けないか、`<id>_`（`rp36_` のように）か、受入 test の名前と `_`（`rb01_scoring_and_eval_packages_are_installed_all_five_versions_match` のように）を付けた物にする。
3 つの形のどれかは file ごとに 1 つにそろえる。
実装の test の名前が在るかは、`rg -n 'fn ([a-z0-9_]+_)?<関数の末尾>\(' native/<crate>/tests/<id>.rs` で 1 件であることで確かめる。

- 置き場は 2 の表と `rust_port.ja.md` の 7 のとおり `native/<crate>/tests/<id>.rs`。
  cargo の command は `native/` で打ち、`flock -w 1800 target/release/.cargo-lock true && cargo test --release --test <id>` の形にする。
  `--ignored` を付ける物は、GPU か Windows か 長い実走を要る test で、表の「閾値」の列に `実走` と書く。
  `target/release/.cargo-lock` が無い最初の build だけは flock を付けずに打ってよい。
- test は `Result<(), Box<dyn std::error::Error>>` を返し、`?` で伝える。
  `unwrap`、`expect`、`unwrap_or*`、assert の中の整数の縮め、実行時の環境変数の読みは書かない。
  `CARGO_MANIFEST_DIR` は `env!` で build 時に受ける。
- RB の test は R0 の席が書いた結果の file を実行時に開いて読む。
  `<base>` は `concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/baseline/")`、`docs/results/` は同じ形で `/../../docs/results/` とする。
  結果の file が無いときは、file の path を名指しした `Err` で落とす。空の入力を「0 件で一致」と読まない。
- 陰性の case は、壊した入力を test の中の `check_*` 関数に渡し、`Err` が返ることを `matches!` で確かめる形にする。
  壊した入力は `native/hayamimi-eval/tests/fixtures/` の file に置いて `include_str!` で渡す。command の文字列に直書きしない。test の中で panic させない。
- 小数は文字列を 10 の -6 乗の整数に読んでから整数で比べる。
  「以内」「以下」の境界は、境界の値と、最小の桁 1 つ分だけ外れた値の両側を行にする。
- パーセンタイルは nearest-rank で、n 個の標本の p% は小さい方から `(p × n + 99) / 100`（整数の割り算）番目とする（`rust_port.ja.md` の 3.1。n = 20 の p95 は 19 番目）。
  n は percentile に渡す標本の数で、`L_asr` は有効な標本の数、`L` と `L_tr` は有効な標本のうち訳が取れた数である。
- oracle の種類：specified＝設計の文、derived＝設計の数と過去の記録から出した値、pseudo＝別に書いた式（awk、`wc`）、implicit＝落ちるか止まるか、partial＝性質だけの検査。
- `hayamimi-eval stream` が出す jsonl の項目名と、`hayamimi-eval record` が `<base>/wire/` に残す置き場の形は、設計に key 名が無い。
  case は意味の名前（`t_end`、`t_final`、`t_tr`、型ごとの件数）で書く。key の綴りと置き場の形は R0 の実装が決め、test はその綴りを定数として 1 か所に持つ。
- `docs/results/baseline_python.md` の行の形は設計に無い。この節が置いた形（節の見出しに ID、表の先頭の列に値）を R0 の席が守る。

### 4.2 RB-1（採点と評価の 5 つが入っている）

test：`native/hayamimi-eval/tests/rb01.rs`。
入力：`<base>/pip_show.txt`（R0 の席が `pip show sacrebleu jiwer simpleder OpenCC pyarrow sherpa-onnx` の出力をそのまま保存した物。6 つ目の `sherpa-onnx` が陽性対照）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RB-1-001 | `<base>/pip_show.txt` の実物 | 5 つの版 | 一致 | `Name:` が sacrebleu、jiwer、simpleder、OpenCC、pyarrow の各ブロックの `Version:` が 2.6.0、4.0.0、0.0.5、1.4.2、25.0.1（specified、RB-1） | 陽性 | `all_five_versions_match` |
| T-RB-1-002 | 001 と同じ file | 5 つ以外を読めること | 一致 | `Name:` が sherpa-onnx のブロックの `Version:` が 1.13.6（specified、RB-1 の陽性対照） | 陽性 | `sherpa_onnx_control_reads_1_13_6` |
| T-RB-1-003 | fixture `rb01_sacrebleu_2_5_9.txt`：sacrebleu だけ `Version: 2.5.9`、残り 4 つは正しい版 | 版が 1 つ違う | `Err` | `check_pip_show_versions` が `Err` で、名指しが sacrebleu の 1 つだけ（derived、001 の file の 1 か所を書き換えた） | 陰性 | `one_wrong_version_names_only_that_package` |
| T-RB-1-004 | fixture `rb01_sacrebleu_2_6_01.txt`：`Version: 2.6.01` | 前方一致の境界（期待値の末尾へ 1 文字足した側） | `Err` | `Err`。版は文字列の完全一致で比べる（specified、RB-1） | 境界、陰性 | `version_is_compared_whole_not_by_prefix` |
| T-RB-1-005 | fixture `rb01_sacrebleu_2_6.txt`：`Version: 2.6` | 前方一致の境界（末尾を 1 文字削った側） | `Err` | `Err`（specified、RB-1） | 境界、陰性 | `shorter_version_is_rejected` |
| T-RB-1-006 | fixture `rb01_not_found.txt`：`WARNING: Package(s) not found: OpenCC, jiwer, pyarrow, sacrebleu, simpleder` | 入れる前の `.venv`（5 つとも無い） | `Err` | `Err` で、名指しが 5 つ全部（specified、RB-1 の陰性対照） | 陰性 | `venv_before_install_names_all_five_as_missing` |
| T-RB-1-007 | fixture `rb01_empty.txt`（0 byte） | 空の入力 | `Err` | `Err`。`Version:` が 0 件の file を「5 つ一致」と読まない（implicit） | 境界、陰性 | `empty_pip_show_is_an_error` |
| T-RB-1-008 | fixture `rb01_lowercase_name.txt`：`Name: opencc`（ほかは 001 の file と同じ） | `Name:` の大文字小文字 | 一致 | `Ok`。pip が `Name:` を正規化名で出す版があるので（NOT VERIFIED。`<py> -m pip show OpenCC` の 1 行で確かめられる）、名前は大文字小文字を区別せず比べる（この席が置いた境界） | 境界、陽性 | `package_name_match_ignores_case` |
| T-RB-1-009 | `<base>/pip_show.txt` が無い | 結果の file の欠け | `Err` | `Err` で、path が `pip_show.txt` で終わる（implicit） | 陰性 | `missing_pip_show_file_names_its_path` |

### 4.3 RB-2（評価の音声と manifest が在る）

test：`native/hayamimi-eval/tests/rb02.rs`。
入力：`testdata/manifests/<set>.json`（6 組：`eval_real`、`eval_real_zhko`、`eval_real_yue`、`eval_diar`、`lat-ja`、`lat-en`）、`testdata/<set>/`（前の 4 組）、結合した wav、`tests/golden/ja/golden.json`。
manifest の各行は wav の相対 path、sha256、Python の manifest が持つ出所の項目を持ち、lat の 2 組はさらに発話の始まりと終わりの sample の位置を持つ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RB-2-001 | `testdata/manifests/` | 6 組 | 6 file | `eval_real.json`、`eval_real_zhko.json`、`eval_real_yue.json`、`eval_diar.json`、`lat-ja.json`、`lat-en.json` が在る（specified、RB-2） | 陽性 | `six_manifests_exist` |
| T-RB-2-002 | 前の 4 組それぞれの manifest と `testdata/<set>/*.wav` | 行の数と wav の数 | 一致 | 組ごとに行数 = wav の数で、どの組も 1 以上。wav の数は test が directory を数えた値で、`ls testdata/<set>/*.wav \| wc -l` と同じ（pseudo、RB-2） | 陽性 | `row_count_equals_wav_count_in_each_set` |
| T-RB-2-003 | `lat-ja.json` と `lat-en.json` | 発話の数 | 23 と 15 | 行数が 23 と 15。式：lat-ja = `tests/golden/ja/` の wav 8 本 + eval_real の ja 15 本（`ls tests/golden/ja/*.wav \| wc -l` = 8）、lat-en = eval_real の en 15 本（derived、3.1 と RB-2） | 陽性 | `lat_sets_have_23_and_15_utterances` |
| T-RB-2-004 | fixture `rb02_lat_ja_22_rows.json`（lat-ja から 1 行落とした物） | 発話の数の下側 | `Err` | `check_lat_ja` が `Err`（23 でない）。001 の実物では `Ok`（derived） | 境界、陰性 | `lat_ja_with_22_utterances_is_rejected` |
| T-RB-2-005 | fixture `rb02_lat_ja_24_rows.json`（1 行足した物） | 発話の数の上側 | `Err` | `Err`（derived） | 境界、陰性 | `lat_ja_with_24_utterances_is_rejected` |
| T-RB-2-006 | `lat-ja.json` の先頭の golden 8 本の行と `tests/golden/ja/golden.json` の `clips[].sha256` | 独立した記録との一致 | 一致 | wav 名ごとに manifest の sha256 = golden.json の sha256（derived、golden.json は 2026-09-02 に記録された） | 陽性 | `golden_part_of_lat_ja_has_the_recorded_sha256` |
| T-RB-2-007 | 文字列 `abc` | sha256 の実装の確かめ | 一致 | test が使う sha256 の関数が `abc` に `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` を返す（specified、FIPS 180-2 の標準の値） | 陽性 | `sha256_of_abc_matches_the_standard_vector` |
| T-RB-2-008 | 6 組の全行 | wav の sha256 | 一致 | 各行の sha256 = test が wav を読んで出した sha256（007 で確かめた関数）。行が 0 の組は 002 が止める（implicit） | 陽性 | `every_row_sha256_matches_its_wav` |
| T-RB-2-009 | `lat-ja.json`、`lat-en.json` の発話の終わりの sample の位置 | 順序と範囲 | 単調増加、最後 ≤ 全 sample 数 | 位置が行の順に厳密に増え、最後の位置 ≤ 結合 wav の sample 数（specified、RB-2 の「発話の終わりの sample の位置」） | 陽性 | `lat_end_positions_increase_and_fit_in_the_wav` |
| T-RB-2-010 | fixture：最後の位置 = 全 sample 数 | 範囲の上端 | `Ok` | `check_end_positions` が `Ok`（derived、009 の式の等号側） | 境界、陽性 | `end_position_equal_to_total_samples_is_accepted` |
| T-RB-2-011 | fixture：最後の位置 = 全 sample 数 + 1 | 範囲の上端の外側 | `Err` | `Err`（derived） | 境界、陰性 | `end_position_one_past_total_samples_is_rejected` |
| T-RB-2-012 | fixture：隣り合う 2 行の位置が同じ | 増加の境界 | `Err` | `Err`。厳密に増える（derived） | 境界、陰性 | `equal_consecutive_end_positions_are_rejected` |
| T-RB-2-013 | `<base>/pin_second/<set>.json`（R0 の席が 2 回目の `--pin` の出力を置く）と `testdata/manifests/<set>.json` | 再実行 | 同じ bytes | 前の 4 組で bytes が同じ（specified、RB-2 の陽性対照） | 陽性 | `second_pin_run_is_byte_identical` |
| T-RB-2-014 | fixture dir `rb02_missing_wav/`：manifest が a.wav と b.wav を持ち、dir には a.wav だけ | wav が 1 本欠ける | `Err` | `Err` で、b.wav を名指し（specified、RB-2 の陰性対照） | 陰性 | `missing_wav_is_named_in_the_error` |
| T-RB-2-015 | fixture：001 の 1 行の sha256 を 1 文字変えた manifest | sha256 の食い違い | `Err` | `Err` で、その行の wav を名指し（implicit） | 陰性 | `wrong_sha256_row_is_named` |
| T-RB-2-016 | 6 組の manifest と `docs/results/baseline_python.md` | 出所の行の番号が無い組の記録 | 名前が在る | manifest に出所の行の番号の項目が無い行を 1 つでも持つ組は、その組の名前が `baseline_python.md` に在る。該当が 0 組なら、`baseline_python.md` にその旨の 1 行が在る（specified、RB-2） | 陽性 | `sets_without_source_row_are_named_in_baseline_summary` |
| T-RB-2-017 | fixture `rb02_summary_without_set_name.md` と、出所の行が無い組の manifest | 記録の欠け | `Err` | `Err` で、組の名前を名指し（specified、RB-2） | 陰性 | `set_missing_from_baseline_summary_is_named` |
| T-RB-2-018 | `testdata/manifests/lat-ja.json` が無い | 結果の file の欠け | `Err` | `Err` で、path が `lat-ja.json` で終わる（implicit） | 陰性 | `missing_manifest_file_names_its_path` |

### 4.4 RB-3（scorecard を Python で取り直す）

test：`native/hayamimi-eval/tests/rb03.rs`。
入力：`<base>/scorecard_record.md`（退避した記録）、`<base>/scorecard_run1.md`、`<base>/scorecard_run2.md`、`docs/results/scorecard.md`、`docs/results/baseline_python.md`。
表は `| lang | clips | LID正解 | 主tier | mean err | mean RTF |` の見出しの下の 5 行で、`mean err` を 10 の -6 乗の整数に読む。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RB-3-001 | `<base>/scorecard_record.md` | 記録の読み取り | 一致 | ja 0.038、en 0.023、zh 0.066、ko 0.081、yue 0.061 の 5 行が読める（specified、RB-3 の記録の値）。読み取りの関数が設計の 5 つの数と合う証拠で、以降の比較の足場 | 陽性 | `record_file_parses_to_the_five_recorded_means` |
| T-RB-3-002 | `scorecard_run1.md` の 5 言語 | 1 回目の run | 差 ≤ 0.005 | 言語ごとに record との差の絶対値 ≤ 5000（10 の -6 乗の単位。specified、RB-3） | 陽性 | `run1_means_within_0_005_of_record` |
| T-RB-3-003 | `scorecard_run2.md` の 5 言語 | 2 回目の run | 差 ≤ 0.005 | 同上（specified、RB-3） | 陽性 | `run2_means_within_0_005_of_record` |
| T-RB-3-004 | fixture `rb03_ja_plus_0_005.md`：ja の `mean err` が 0.043、ほかは記録どおり | 差の上端（差 = 0.005） | `Ok` | `check_means_against_record` が `Ok`（derived、0.043 − 0.038 = 0.005） | 境界、陽性 | `difference_of_exactly_0_005_is_accepted` |
| T-RB-3-005 | fixture `rb03_ja_plus_0_006.md`：ja が 0.044 | 差の上端の外側（最小の桁 0.001 だけ外れる） | `Err` | `Err` で ja を名指し（derived、0.044 − 0.038 = 0.006） | 境界、陰性 | `difference_of_0_006_is_rejected` |
| T-RB-3-006 | fixture `rb03_ja_minus_0_006.md`：ja が 0.032 | 差の下側の外側 | `Err` | `Err`（derived、0.038 − 0.032 = 0.006）。差は絶対値で見る | 境界、陰性 | `difference_below_record_by_0_006_is_rejected` |
| T-RB-3-007 | fixture `rb03_no_yue_row.md`：yue の行を落とした表 | 言語の欠け | `Err` | `Err` で yue を名指し（implicit） | 陰性 | `missing_language_row_is_named` |
| T-RB-3-008 | run1 と run2 の本文 | yue の正規化の印 | 在る | 両方に `yueはt2s正規化` が在る（specified、RB-3） | 陽性 | `runs_report_t2s_normalisation_for_yue` |
| T-RB-3-009 | fixture `rb03_run_without_opencc.md`：本文が `yueは生CER: opencc無し`、yue の `mean err` が記録からずれた値 | OpenCC の無い `.venv` の run | `Err` | `Err`。`opencc無し` の印を理由に名指し（specified、RB-3 の陰性対照） | 陰性 | `run_without_opencc_is_rejected` |
| T-RB-3-010 | run1 と run2 の本文 | LID の誤判定の節 | 在る | `LID誤判定の内訳` の見出しが両方に在る（specified、RB-3 の陽性対照） | 陽性 | `runs_contain_the_lid_misjudgement_section` |
| T-RB-3-011 | `baseline_python.md` の RB-3 の節と run1、run2 | 2 回の差の記録 | 一致 | 言語ごとに、言語名と同じ行に run1 と run2 の差の絶対値を小数 3 桁で書いた値が在る。値は test が run1 と run2 から出す（pseudo、RB-3 の「2 回の差を書く」） | 陽性 | `baseline_summary_records_the_difference_between_runs` |
| T-RB-3-012 | fixture `rb03_summary_without_difference.md` | 差の記録の欠け | `Err` | `Err` で、欠けた言語を名指し（implicit） | 陰性 | `summary_without_run_difference_is_rejected` |
| T-RB-3-013 | `docs/results/scorecard.md` と `<base>/scorecard_record.md` | 書き戻し（行の終わりを除く） | 行の終わりを除いて同じ bytes | 2 つの file の bytes の `\r\n` だけを `\n` にそろえてから比べ、同じ。単独の `\r` はそろえずに残す。そろえるのは、同じ木を Windows の git（`core.autocrlf=true`）と WSL の git（設定なし）が書き、checkout の行の終わりが木ごとに違うからである（derived、書き戻しの定義。R0 の席が打つ `git diff --ignore-cr-at-eol --quiet docs/results/scorecard.md` の rc 0 と同じ事実を別の式で確かめる） | 陽性 | `scorecard_md_is_restored_up_to_line_endings` |
| T-RB-3-014 | fixture `rb03_restored_a.md` と `rb03_restored_b.md`：行の終わり以外の 1 byte（3 byte 目の `a` と `b`）だけ違う組 | 書き戻しの境界（行の終わり以外の 1 byte の差） | `Err` | `Err`（derived） | 境界、陰性 | `one_byte_difference_in_restored_scorecard_is_rejected` |
| T-RB-3-015 | `<base>/scorecard_run1.md` が無い | 結果の file の欠け | `Err` | `Err` で path 名指し（implicit） | 陰性 | `missing_scorecard_run_names_its_path` |
| T-RB-3-016 | test の中で組んだ 2 つの bytes：`# a\r\n# b\r\n` と `# a\n# b\n` | CRLF と LF で中身が同じ | `Ok` | 013 と同じ比べ方で `Ok`（specified、RB-3 の「行の終わりを除く」）。bytes は fixture の file でなく test の中で組む。fixture の file は checkout の行の終わりの設定で bytes が変わるからである（4.1 の「壊した入力は fixtures/ の file に置く」の例外） | 境界、陽性 | `crlf_and_lf_with_the_same_content_are_accepted` |
| T-RB-3-017 | test の中で組んだ 2 つの bytes：`# a\rb\n` と `# ab\n` | 単独の `\r` の差 | `Err` | 013 と同じ比べ方で `Err`。そろえるのは `\r\n` だけで、単独の `\r` は中身として比べる（specified、RB-3）。bytes を test の中で組む理由は 016 と同じ | 境界、陰性 | `lone_carriage_return_difference_is_rejected` |

### 4.5 RB-4（golden 8 本を Python で取り直す）

test：`native/hayamimi-eval/tests/rb04.rs`。
入力：`<base>/golden_check.log`（R0 の席が `<py> scripts/make_ja_golden.py --check` の標準出力をそのまま保存した物）、`<base>/golden_check.rc`（終了 code を 1 行で書いた物）、`tests/golden/ja/` の wav の一覧。
`--check` が比べるのは `finals` の文字列で、CER ではない。finals が同じなら同じ参照文に対する CER も同じなので、一致の判定はこの出力の `DIFF` の行が 0 件であることで行う。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RB-4-001 | `golden_check.log` の `  <wav>  finals=` で始まる行 | 8 本の名前 | 一致 | 名前の集合が ja_014、ja_024、ja_033、ja_036、ja_044、ja_079、ja_085、ja_090 と同じ。集合は `ls tests/golden/ja/*.wav` を test が数えた物とも一致（pseudo、RB-4 の陽性対照） | 陽性 | `log_names_all_eight_golden_clips` |
| T-RB-4-002 | 同じ log の最後の統計の行 | 差の件数 | `0 of 8 clips differ` | 行が在る（specified、`make_ja_golden.py` の `--check` の出力） | 陽性 | `log_reports_zero_of_eight_clips_differ` |
| T-RB-4-003 | 同じ log | `DIFF` の行 | 0 件 | 行頭が `DIFF ` の行が 0 件。002 の行が在るので 0 件が「空の log」の 0 ではない（specified、RB-4） | 陽性 | `log_has_no_diff_lines` |
| T-RB-4-004 | `golden_check.rc` | 終了 code | 0 | 内容が `0`（specified、RB-4） | 陽性 | `check_exit_code_is_zero` |
| T-RB-4-005 | fixture `rb04_two_diffs.log`：`DIFF ja_085.wav` と `DIFF ja_090.wav` と `2 of 8 clips differ`、rc 1（ITN を外した設定） | 差が 2 本 | `Err` | `check_golden_log` が `Err` で ja_085 と ja_090 の 2 つを名指し（specified、RB-4 の陰性対照） | 陰性 | `itn_off_log_names_ja_085_and_ja_090` |
| T-RB-4-006 | fixture `rb04_seven_clips.log`：8 本のうち 1 本の行が無く、統計が `0 of 7 clips differ` | 本数の下側 | `Err` | `Err`（derived、8 本でない） | 境界、陰性 | `log_with_seven_clips_is_rejected` |
| T-RB-4-007 | fixture `rb04_nine_clips.log`：9 本 | 本数の上側 | `Err` | `Err`（derived） | 境界、陰性 | `log_with_nine_clips_is_rejected` |
| T-RB-4-008 | fixture `rb04_empty.log`（0 byte） | 空の log | `Err` | `Err`。0 本で差 0 件の log を成功と読まない（implicit） | 境界、陰性 | `empty_golden_log_is_an_error` |
| T-RB-4-009 | `golden_check.log` が無い | 結果の file の欠け | `Err` | `Err` で path 名指し（implicit） | 陰性 | `missing_golden_log_names_its_path` |

### 4.6 RB-5（Python の遅れ `L` と `L_asr`）

test：`native/hayamimi-eval/tests/rb05.rs`。
入力：`<base>/lat_ja_0.35_<n>.jsonl`（`--min-silence` 0.35 の run を n = 1、2、…）、`<base>/lat_ja_0.35_<n>.events.jsonl`（その run の WS の event）、`<base>/server_0.35_<n>.out.log`（その run の Python の stdout）、`docs/results/baseline_python.md`。
発話ごとの標本は `t_end`、`t_final`、`t_tr`（無ければ空）、`t_final` を出した `final` の `latency_ms`（`L_dec`）、繰り返しの番号、発話の番号を持つ。
`L = t_tr − t_end`、`L_asr = t_final − t_end`、`L_tr = t_tr − t_final`（3.1）。
標本の枠は 23 発話 × 5 回 = 115 で、`枠 = 有効 + final なし + 終端不正`（3.1）。
有効は `final` が在り `L_asr − L_dec ≥ min_silence` の標本で、`min_silence` は run に渡した 0.35 秒（350 ms）である。
`L_asr` の標本は有効な標本、`L` と `L_tr` の標本は有効な標本のうち訳が取れた物である。
test は有効な標本の数を定数に持たない。数えるのは枠の和と、3 種の仕分けの式である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RB-5-001 | `<base>/lat_ja_0.35_*.jsonl` | run の数 | 2 以上 | file が 2 つ以上（specified、RB-5） | 陽性 | `at_least_two_runs_exist` |
| T-RB-5-002 | fixture dir `rb05_one_run/`：run が 1 つ | run の数の下側 | `Err` | `check_run_count` が `Err`（derived、2 未満） | 境界、陰性 | `single_run_is_rejected` |
| T-RB-5-003 | 各 run の jsonl | 標本の枠の分割 | 和が枠 | 各 run で、(発話, 繰り返し) の組が 23 × 5 通りで重複なく、有効 + final なし + 終端不正 = 23 × 5。仕分けは `t_final` が空なら final なし、`L_asr − L_dec < 350 ms` なら終端不正、それ以外が有効（specified、3.1 の分割式。数は実物から数え、定数に置かない） | 陽性 | `each_run_splits_its_frame_into_valid_no_final_and_bad_end` |
| T-RB-5-004 | 各 run の jsonl | `L` の標本と取れない有効な標本 | 和が有効 | `L` を持つ標本の数 + 訳の取れない有効な標本の数 = 有効な標本の数（derived、RB-5 の注。取れない標本は `L` の標本から外す） | 性質 | `translated_plus_untranslated_valid_samples_equal_valid_samples` |
| T-RB-5-005 | 各 run の有効な標本と、同じ run の `events.jsonl` | 有効の判定式 | 全件で成り立つ | 有効な標本の全件で `L_asr − L_dec ≥ 350 ms`。`L_dec` は標本の値でなく、`t_final` と同じ時刻の WS の `final` event の `latency_ms` から取り直す（pseudo、別の経路で取った値。標本の値と一致しない標本が 1 件でもあれば `Err`） | 性質 | `valid_samples_satisfy_asr_minus_dec_at_least_min_silence` |
| T-RB-5-006 | fixture `rb05_known_valid.jsonl`：min_silence 0、有効な標本 115 行（`L_asr` が 1 から 115 ms、`L_dec` が 0）に、終端不正 2 行（`L_asr` 500 ms、`L_dec` 800 ms）と final なし 3 行を足した 120 行 | 有効な標本の数 n = 115 の percentile | p50 と p95 | n = 115、p50 = 58、p95 = 110。式：`(50 × 115 + 99) / 100` = 58、`(95 × 115 + 99) / 100` = 110（specified、3.1 の nearest-rank）（pseudo、awk の `int((95*115+99)/100)`）。終端不正を数えに入れると n = 117、p50 = 59、p95 = 112 になり落ちる | 陽性、陰性 | `percentiles_of_115_valid_samples_are_58_and_110_and_skip_the_others` |
| T-RB-5-007 | fixture：標本 20 個（1 から 20 ms） | p95 の境界（95 × 20 が 100 で割り切れる） | 19 番目 | p95 = 19。式：`(95 × 20 + 99) / 100` = 19（derived） | 境界 | `p95_of_20_samples_is_the_19th` |
| T-RB-5-008 | fixture：標本 21 個（1 から 21 ms） | p95 の境界の隣 | 20 番目 | p95 = 20。式：`(95 × 21 + 99) / 100` = 20（derived） | 境界 | `p95_of_21_samples_is_the_20th` |
| T-RB-5-009 | fixture：標本 1 個（7 ms） | 最小の標本 | 7 | p50 = p95 = 7（derived） | 境界 | `single_sample_percentiles_are_that_sample` |
| T-RB-5-010 | fixture：標本 0 個 | 空 | `Err` | `Err`。0 個の percentile を 0 と読まない（implicit） | 境界、陰性 | `percentile_of_no_samples_is_an_error` |
| T-RB-5-011 | 各 run の jsonl と `baseline_python.md` の RB-5 の節 | 要約の値 | 一致 | run ごと、`L`、`L_asr`、`L_tr` の p50 と p95（整数 ms）が、test が jsonl から出した値と同じ（pseudo、test の中に別に書いた nearest-rank の関数） | 陽性 | `baseline_summary_matches_recomputed_percentiles` |
| T-RB-5-012 | fixture：ある発話の 5 回の `L` が 10、30、20、50、40 | 発話ごとの 5 回の中央値 | 30 | 中央値 = 並べて 3 番目 = 30（specified、3.1 の「5 回の中央値」） | 陽性 | `median_of_five_runs_is_the_third_sorted_value` |
| T-RB-5-013 | `baseline_python.md` の RB-5 の節 | 発話ごとの中央値の表 | 23 行 | 23 発話の行が在り、値が test の出した中央値と同じ（pseudo） | 陽性 | `baseline_summary_lists_23_per_utterance_medians` |
| T-RB-5-014 | fixture：`L_tr` が負の標本を 1 つ含む jsonl（`t_tr` < `t_final`） | `L_tr` が負 | 1 件 | `L_tr` が負の標本の数 = 1 で、`baseline_python.md` の同じ数が 1 以上のとき一致を要る（derived、3.1。Python の対応づけで負になるかは実測まで不明） | 性質 | `negative_l_tr_samples_are_counted` |
| T-RB-5-015 | fixture event 列：final(u1)、translation(a)、final(u2)、final(u3)、translation(b) | Python の対応づけ | 規則どおり | u1 → a、u2 は取れない、u3 → b（specified、RB-5 の注） | 陽性 | `translation_pairs_with_the_preceding_final_until_the_next_final` |
| T-RB-5-016 | fixture event 列：final(u1)、translation(a)、translation(a2)、final(u2) | 1 つの final の後に訳が 2 つ | 最初の 1 つ | u1 → a（a2 は使わない。specified、RB-5 の「最初に受けた」） | 境界 | `first_translation_after_a_final_wins` |
| T-RB-5-017 | fixture event 列：translation(x)、final(u1) | final より前の訳 | 対応づけない | x はどの発話にも付かず、u1 は取れない（derived、RB-5 の「final を受けた後」） | 境界、陰性 | `translation_before_any_final_is_not_paired` |
| T-RB-5-018 | 2 つの run の `L` の p50 | 回の間の差 | 記録 | `baseline_python.md` に run 1 と run 2 の `L` の p50 の差が 1 行で在り、値は test が出した差と同じ（pseudo、RB-5） | 陽性 | `p50_difference_between_runs_is_recorded` |
| T-RB-5-019 | fixture `rb05_ws_122_log_123`：WS の `final` が 122 本、log の `latency=` が 123 本（平均は同じ） | 本数の差 1 | `Err` | `check_final_latency_agreement` が `Err` で、2 つの本数を名指し。本数が同じことが先で、平均は本数が同じときだけ比べる。発話の最後の `final` だけを数えた平均は母集団が違うので比べない（specified、RB-5 の陽性対照） | 境界、陰性 | `ws_and_log_final_counts_must_be_equal` |
| T-RB-5-020 | fixture：WS と log の本数が 123 で同じ、平均が 266.800000 と 267.300000（差 0.5 ms）、266.800000 と 267.300001（差 0.5 ms より 10 の -6 乗 ms だけ大きい） | 平均の差の上端 | 差 0.5 ms 以内 | 前者は `Ok`、後者は `Err`（specified、RB-5 の「差が 0.5 ms 以内」。10 の -6 乗 ms の整数で比べる） | 境界 | `final_latency_mean_difference_edge_is_0_5_ms` |
| T-RB-5-021 | 実物の各 run の `events.jsonl` の `final` event の `latency_ms` と、`server_0.35_<n>.out.log` の `latency=<値>ms` | 陽性対照 | 本数が同じで差 0.5 ms 以内 | WS と log を別々に集計し（pseudo、2 つの経路。jq と awk）、本数が同じで 1 本以上、平均の差が 0.5 ms 以内。log は整数 ms で出るので、丸めの誤差が平均に残る（下の注） | 陽性 | `ws_and_log_final_latency_means_agree_within_0_5_ms` |
| T-RB-5-022 | fixture `rb05_no_final.jsonl`：`t_final` が全標本で空 | `/ingest` に繋がなかった run | `Err` | `Err` で `t_final` が 0 件と言う（specified、RB-5 の陰性対照） | 陰性 | `run_without_final_events_is_an_error` |
| T-RB-5-023 | 閉じた port（test が bind して port を読み、listener を落とした port）へ `hayamimi-eval stream` の接続部分を向ける | 繋がらない | 接続の `Err` | 接続の error の型で返る（implicit）。実装は `/ingest` の WebSocket を張れないときに値を作らない | 陰性 | `stream_against_closed_ingest_port_is_a_connect_error` |
| T-RB-5-024 | `<base>/lat_ja_0.35_1.jsonl` が無い | 結果の file の欠け | `Err` | `Err` で path 名指し（implicit） | 陰性 | `missing_latency_run_names_its_path` |
| T-RB-5-025 | fixture `rb05_split_boundary.jsonl`：min_silence 350 ms。標本 a は `L_asr − L_dec` が 350.000 ms、標本 b は 349.999 ms、標本 c は `t_final` が空 | 有効の判定式の境界（等号の側と、最小の桁 10 の -6 乗 ms だけ外れた側） | 仕分けが 1 / 1 / 1 | a が有効、b が終端不正、c が final なし。3 種がどれも空でない（specified、3.1 の `≥ min_silence`） | 境界、陽性、陰性 | `asr_minus_dec_of_exactly_min_silence_is_valid_and_one_microsecond_less_is_bad_end` |
| T-RB-5-026 | fixture：WS の `final` が 0 本、log の `latency=` が 0 本 | 両方が空 | `Err` | `Err` で 0 本と言う。0 本と 0 本を「本数が同じ」と読まない（implicit） | 境界、陰性 | `zero_finals_on_both_sides_is_an_error_not_an_agreement` |
| T-RB-5-027 | fixture `rb05_frame_114.jsonl`：(発話, 繰り返し) の組が 114 通りの run | 枠の下側 | `Err` | `Err` で 114 と 23 × 5 を名指し（derived、003 の式。仕分けの和が枠に足りない run を通さない） | 境界、陰性 | `run_with_a_missing_sample_is_rejected_by_the_frame_check` |
| T-RB-5-028 | fixture `rb05_frame_duplicate.jsonl`：同じ (発話, 繰り返し) の組が 2 行在り、行の数は 23 × 5 | 組の重複 | `Err` | `Err` で重複した組を名指し。行の数が枠と同じでも、組が 23 × 5 通りに揃わなければ通さない（derived、003 の式） | 境界、陰性 | `run_with_a_duplicated_pair_is_rejected_by_the_frame_check` |

仕分けを実物で数えた値と、WS と log の平均（期待値ではなく、3 種が実物で空でないことと、2 つの経路が別々に出した値の対照）。
`testdata/baseline/` で `lat_ja_0.35_<n>.jsonl`（n = 1、2）に次を打った。

```
jq -s -c '{frame: length, valid: ([.[] | select(.t_final_us != null and ((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) >= 350)] | length), no_final: ([.[] | select(.t_final_us == null)] | length), bad_end: ([.[] | select(.t_final_us != null and ((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) < 350)] | length)}' lat_ja_0.35_<n>.jsonl
{"frame":115,"valid":100,"no_final":12,"bad_end":3}
{"frame":115,"valid":99,"no_final":13,"bad_end":3}
```

WS は `jq -s -c '[.[] | select(.event.type == "final") | .event.latency_ms] | {n: length, mean: (add / length)}' lat_ja_0.35_<n>.events.jsonl`、log は `grep -oE 'latency=[0-9.]+ms' server_0.35_<n>.out.log | sed -E 's/latency=([0-9.]+)ms/\1/' | awk '{s += $1; c++} END {print c, s / c}'` で集計した。

| run | WS の本数と平均（ms） | log の本数と平均（ms） | 差（ms） |
|---|---|---|---|
| 1 | 123、266.8027 | 123、266.7967 | 0.0060 |
| 2 | 123、267.3578 | 123、267.3577 | 0.0001 |

Python の log は `latency=<値>ms` を整数 ms で出す（`scripts/realtime_transcribe.py` の `latency={latency_ms:.0f}ms`）。
1 本の丸めの誤差は 0.5 ms 以内なので、平均の差も 0.5 ms を超えない。読んだ 2 run の差は 0.0060 ms と 0.0001 ms で、打ち消し合って 0.5 ms よりずっと小さい。

### 4.7 RB-6（`--min-silence` の値）

test：`native/hayamimi-eval/tests/rb06.rs`。
入力：`<base>/lat_ja_<s>_<n>.jsonl`（`<s>` は 0.20、0.25、0.30、0.35）、`docs/results/baseline_python.md` の RB-6 の節。
表の行は先頭の列が `<s>`（`0.20` ほか）、続く列が `L_asr` の p50、p95、ja の CER である。
`L_asr` は各行の run をその run の `min_silence`（`<s>` 秒）で仕分けた有効な標本から数える（RB-5 の導入と同じ仕分け）。CER は全標本から出す。
ja の CER は 3.2 の `hayamimi-eval` が `final` の文字列と参照文から出す。正規化は NFKC、句読点と空白を除く、Levenshtein 距離を参照文の長さで割る（`scripts/eval_accuracy.py` の `normalize_ja` と `cer_ja` と同じ）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RB-6-001 | `<base>/` の jsonl | 4 つの値の run | 4 組 | `<s>` が 0.20、0.25、0.30、0.35 の 4 組それぞれに file が 1 つ以上（specified、RB-6） | 陽性 | `four_min_silence_values_have_runs` |
| T-RB-6-002 | `baseline_python.md` の RB-6 の表 | 行の数 | 4 行 | 先頭の列が 0.20、0.25、0.30、0.35 の 4 行（specified、RB-6） | 陽性 | `table_has_the_four_rows` |
| T-RB-6-003 | fixture `rb06_three_rows.md`：0.25 の行が無い表 | 行の欠け | `Err` | `Err` で 0.25 を名指し（specified、RB-6 の陰性対照） | 陰性 | `table_missing_a_row_is_rejected` |
| T-RB-6-004 | 表の各行と jsonl | 値の一致 | 一致 | 行ごとに `L_asr` の p50 と p95 が、jsonl の有効な標本（`L_asr − L_dec ≥ <s>` 秒）から nearest-rank で出した値と同じ（pseudo。nearest-rank の関数は RB-5 の 006 から 008 で確かめた物）。final なしと終端不正の標本は数えない | 陽性 | `table_values_match_recomputed_l_asr_percentiles_over_valid_samples` |
| T-RB-6-005 | CER の組 {0.20: 0.050、0.25: 0.040、0.30: 0.036、0.35: 0.036} | 名指しの規則（同じ値が等号で通る） | 0.30 | `choose_min_silence` が 0.30。式：CER(s) ≤ CER(0.35) を満たす最小の s（specified、RB-6） | 境界、陽性 | `tie_with_the_default_cer_is_accepted` |
| T-RB-6-006 | CER の組 {0.20: 0.036、0.25: 0.036、0.30: 0.036、0.35: 0.036} | 全部同じ | 0.20 | 0.20（specified、RB-6） | 境界 | `all_equal_cer_names_the_smallest_value` |
| T-RB-6-007 | CER の組 {0.20: 0.037、0.25: 0.037、0.30: 0.037、0.35: 0.036} | 他の値が 0.001 だけ超える | 0.35 | 0.35。0.037 は 0.036 を超えるので選ばない（specified、RB-6 の「超えない最小」。最小の桁 0.001 の外側） | 境界 | `value_one_step_above_default_cer_is_not_chosen` |
| T-RB-6-008 | `baseline_python.md` の RB-6 の節の名指し | 名指しの数と値 | 1 つ | 名指しされた値が 1 つで、005 から 007 の関数が表の CER から出した値と同じ（specified、RB-6） | 陽性 | `baseline_names_exactly_one_value_matching_the_rule` |
| T-RB-6-009 | fixture `rb06_two_values_named.md`：0.25 と 0.30 の 2 つを名指しした節 | 名指しの数の上側 | `Err` | `Err`（specified、RB-6 の「1 つ」） | 境界、陰性 | `two_named_values_are_rejected` |
| T-RB-6-010 | fixture `rb06_no_value_named.md` | 名指しの数の下側 | `Err` | `Err`（specified） | 境界、陰性 | `no_named_value_is_rejected` |
| T-RB-6-011 | 表の 0.35 の行と RB-5 の 1 回目の run の有効な標本の値 | 陽性対照（同じ桁） | 比が 10 倍未満 | 0.35 の `L_asr` の p50 と p95 が、RB-5 の 1 回目の run の同じ値の 1/10 より大きく 10 倍より小さい（partial。設計の「同じ桁」を、整数 ms の比で 10 倍の内側と読んだ） | 陽性 | `default_row_is_within_a_factor_of_ten_of_rb05` |
| T-RB-6-012 | 数の組 100 と 999、100 と 1000 | 同じ桁の境界 | 比 10 | (100, 999) は `Ok`、(100, 1000) は `Err`（derived、011 の式） | 境界 | `factor_of_ten_boundary_between_999_and_1000` |
| T-RB-6-013 | 参照文 `こんにちは`、`final` の文字列 `こんにちわ` | CER の関数、置換 1 文字 | 0.2 | CER = 0.2。式：距離 1 ÷ 長さ 5（derived、`normalize_ja` と `cer_ja` の定義。pseudo、awk の `1/5`） | 陽性 | `cer_one_substitution_in_five_is_0_2` |
| T-RB-6-014 | 参照文 `こんにちは。`、`final` の文字列 `こんにちは` | 句読点は数えない | 0 | CER = 0（derived、`normalize_ja` が句読点を除く） | 境界 | `cer_ignores_punctuation` |
| T-RB-6-015 | 参照文 `ＡＢＣ`（全角）、`final` の文字列 `ABC` | NFKC | 0 | CER = 0（derived、`normalize_ja` が NFKC を掛ける） | 境界 | `cer_applies_nfkc` |
| T-RB-6-016 | 参照文 `こんにちは`、`final` の文字列 空 | hyp が空 | 1 | CER = 1。式：距離 5 ÷ 長さ 5（derived） | 境界 | `cer_of_empty_hypothesis_is_1` |
| T-RB-6-017 | 参照文 空（句読点だけ `。` を含む） | ref が正規化後に空 | 0 | CER = 0（derived、`cer_ja` が長さ 0 のとき 0）。0 で割らない | 境界 | `cer_of_empty_reference_is_0_not_a_division` |
| T-RB-6-018 | `<base>/lat_ja_0.20_1.jsonl` が無い | 結果の file の欠け | `Err` | `Err` で path 名指し（implicit） | 陰性 | `missing_sweep_run_names_its_path` |
| T-RB-6-019 | fixture `rb06_sweep_bad_end.jsonl`：min_silence 0.30。有効な 5 標本（`L_asr` が 1000、1100、1200、1300、1400 ms、`L_dec` が 0）と、終端不正の 1 標本（`L_asr` 90 ms、`L_dec` 400 ms） | 終端不正を数えに入れるか | 有効な標本だけ | p50 = 1200。式：有効な 5 標本の `(50 × 5 + 99) / 100` = 3 番目。終端不正を入れると 6 標本の 3 番目で 1100 になり落ちる（derived、手計算） | 陽性、陰性 | `bad_end_sample_is_left_out_of_the_sweep_l_asr_percentiles` |
| T-RB-6-020 | 4 つの値の run の jsonl | 有効な標本で数えた値（実物） | 4 行 | 有効な標本の数 n と `L_asr` の p50 と p95（整数 ms）が、0.20 は 100、1010、1542、0.25 は 100、995、1635、0.30 は 100、1104、1724、0.35 は 100、1185、1779。0.35 の p50 は全標本で数えた 1161 ではない。値は別の式（jq、下の注）が出した物で、test が定数に持つ（pseudo）。R0 で測り直したら注の値と同じ手番で直す | 陽性、陰性 | `sweep_rows_match_the_jq_recount_over_valid_samples` |

有効な標本で数え直した値（020 の定数の出所）を、`testdata/baseline/lat_ja_<s>_1.jsonl` に打った。
`<ms>` は `<s>` を ms にした値（0.20 なら 200）、nearest-rank は `(p × n + 99) / 100` 番目である。
下の出力の左の列は打った `<s>` で、jq の出力ではない。

```
$ jq -s -c --argjson ms <ms> '[.[] | select(.t_final_us != null and ((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) >= $ms) | ((.t_final_us - .t_end_us) / 1000)] | sort | {n: length, p50: .[((50 * length + 99) / 100 | floor) - 1], p95: .[((95 * length + 99) / 100 | floor) - 1]}' lat_ja_<s>_1.jsonl
0.20  {"n":100,"p50":1009.891,"p95":1541.961}
0.25  {"n":100,"p50":994.607,"p95":1634.766}
0.30  {"n":100,"p50":1104.03,"p95":1723.744}
0.35  {"n":100,"p50":1185.466,"p95":1779.09}
```

対照として、`select` を `.t_final_us != null` だけにすると、0.30 は n が 101 で p50 と p95 は同じ、0.35 は n が 103 で p50 が 1161.315、p95 が 1779.09 になる。
有効の判定が動かすのは 0.35 の p50 だけで、1161 が 1185 になる。0.35 の run 2 は有効が 99 標本で p50 が 1132.326 である。

### 4.8 RB-7（訳の chrF と DER の基準）

test：`native/hayamimi-eval/tests/rb07.rs`。
入力：`<base>/chrf_eval.log`（`eval_translate.py` の標準出力）、`<base>/der_eval.log`（`eval_diar.py` の標準出力）、`<base>/chrf_fixture.tsv`、`<base>/chrf_control.tsv`（R0 の席が、参照訳を hyp にした corpus の値と原文を hyp にした corpus の値を言語ごとに 1 行で書く）。
`chrf_eval.log` の末尾の行は `[<backend>] <lang>: chrF=47.57  n=50  mean=<ms> ms/line`、`der_eval.log` の行は `[<meeting>] DER=16.8%` と `=== mean DER over 5 meeting(s): <値>% (collar=0.25s) ===` である。
DER の run は `der_eval.log` と `der_eval_run<n>.log`（n = 2、3、…）で、`der_eval_default_method.log`（`--method` を付けない run）は数えない。
`--method refine_diarize` の run の会議別の行は `diar=<秒>s over <n> groups (diar_rtf=<値>)` を持ち、`--method` を付けない run の行は持たない（`scripts/eval_diar.py` の `diar_time_s` を持つ result の分岐）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RB-7-001 | `chrf_eval.log` の 4 言語 | 記録との差 | ≤ 0.01 | en 47.57、zh 20.19、ko 25.20、es 42.09 に対し、言語ごとに差の絶対値 ≤ 0.01（specified、RB-7） | 陽性 | `chrf_within_0_01_of_record` |
| T-RB-7-002 | fixture `rb07_chrf_en_plus.log`：en が 47.58 と 47.59 の 2 つ | 差の上端 | 47.58 は `Ok`、47.59 は `Err` | `check_chrf` の戻り（derived、0.01 の等号と最小の桁 0.01 の外側） | 境界 | `chrf_upper_edge_is_record_plus_0_01` |
| T-RB-7-003 | fixture `rb07_chrf_en_minus.log`：en が 47.56 と 47.55 | 差の下端 | 47.56 は `Ok`、47.55 は `Err` | 同上（derived） | 境界 | `chrf_lower_edge_is_record_minus_0_01` |
| T-RB-7-004 | fixture `rb07_chrf_no_es.log`：es の行が無い | 言語の欠け | `Err` | `Err` で es を名指し（implicit） | 陰性 | `chrf_log_missing_a_language_is_named` |
| T-RB-7-005 | `<base>/der_eval.log` と `der_eval_run<n>.log` | DER の run の数 | 3 以上 | file が 3 つ以上で、どの run も `check_der` が `Ok`（5 会議の平均を持つ）。`diarization.md` の 13.9% とは比べない（specified、RB-7 と設計 8 の R0 の「DER は 3 回以上」） | 陽性 | `at_least_three_der_runs_exist` |
| T-RB-7-006 | fixture：`mean DER` の行を持つ run が 2 つ | run の数の下側 | `Err` | `check_der_run_count` が `Err` で 2 と 3 を名指し。2 回では幅を決めない（specified、RB-7） | 境界、陰性 | `two_der_runs_are_rejected` |
| T-RB-7-007 | fixture：`mean DER` の行を持つ run が 3 つ | run の数の等号 | `Ok` | `Ok`（specified、RB-7） | 境界、陽性 | `three_der_runs_are_accepted` |
| T-RB-7-008 | fixture：`over 4 meeting(s)` の行 | 会議の数の下側 | `Err` | `Err`（specified、RB-7 の「AMI の 5 会議」） | 境界、陰性 | `four_meetings_are_rejected` |
| T-RB-7-009 | fixture：`over 6 meeting(s)` の行 | 会議の数の上側 | `Err` | `Err`（specified） | 境界、陰性 | `six_meetings_are_rejected` |
| T-RB-7-010 | `der_eval.log` の会議別の `DER=` の 5 行と平均の行 | 内部の整合 | 差 ≤ 0.1 | 5 つの会議別の DER（小数 1 桁）の平均と、平均の行の値の差 ≤ 0.1。丸めが 2 回入るので 0.1（partial） | 性質 | `meeting_ders_average_to_the_printed_mean` |
| T-RB-7-011 | `chrf_fixture.tsv` | 行の数 | 言語ごとに 50 + 1 | 4 言語それぞれに、文ごとの行が 50 と corpus の行が 1。全体で 204 行（derived、`--n 50` と RB-7 の「文ごとの値と corpus の値」） | 陽性 | `fixture_has_50_sentences_and_one_corpus_row_per_language` |
| T-RB-7-012 | fixture `rb07_fixture_no_corpus_row.tsv`：zh の corpus の行が無い | corpus の行の欠け | `Err` | `Err` で zh を名指し（implicit） | 陰性 | `fixture_missing_a_corpus_row_is_named` |
| T-RB-7-013 | 参照 `The cat sat on the mat.`（en）、es の `Hola mundo cruel.` | 空白で語を分ける言語の degrade | 先頭の語を落とす | hyp が `cat sat on the mat.` と `mundo cruel.`（specified、RB-7 の `--degrade`） | 陽性 | `degrade_drops_the_first_word_of_space_separated_text` |
| T-RB-7-014 | 参照 `你好世界。`（zh） | 空白で語を分けない言語の degrade | 先頭の 1 文字を落とす | hyp が `好世界。`（specified、RB-7） | 陽性 | `degrade_drops_the_first_character_of_zh` |
| T-RB-7-015 | 参照 `Yes`（1 語） | 語が 1 つ | 空の hyp | hyp が空の文字列（derived、先頭の語を落とすと何も残らない） | 境界 | `degrade_of_a_one_word_sentence_is_empty` |
| T-RB-7-016 | `chrf_control.tsv` の参照訳を hyp にした値 | 陽性対照 | 100 | 4 言語とも 100.0（specified、RB-7） | 陽性 | `reference_as_hypothesis_scores_100` |
| T-RB-7-017 | `chrf_control.tsv` の原文を hyp にした値と記録の chrF | 陰性対照 | 記録未満 | 言語ごとに、原文を hyp にした値 < 記録の値（specified、RB-7 の「値を割る」） | 陰性 | `source_as_hypothesis_scores_below_record` |
| T-RB-7-018 | fixture `rb07_control_above_record.tsv`：zh の原文 hyp の値が 25.00（記録 20.19 より上） | 陰性対照が働かない側 | `Err` | `Err` で zh を名指し（derived、017 の式の外側） | 境界、陰性 | `source_as_hypothesis_above_record_is_flagged` |
| T-RB-7-019 | `<base>/chrf_eval.log` が無い | 結果の file の欠け | `Err` | `Err` で path 名指し（implicit） | 陰性 | `missing_chrf_log_names_its_path` |
| T-RB-7-020 | 実物の DER の run の会議別の行と、fixture `rb07_der_default_method.log`（`--method` を付けない run の行で、`diar_rtf=` が無い） | `--method refine_diarize` の run の判別 | refine_diarize の run だけ | 実物の全 run は `Ok`、fixture は `Err` で `--method` を付けない run と言う（partial、会議別の行の `diar_rtf=` の有無。出力の観察で、script の仕様の保証ではない） | 陽性、陰性 | `der_run_without_diar_rtf_rows_is_not_a_refine_diarize_run` |
| T-RB-7-021 | `baseline_python.md` の RB-7 の節の「DER 平均」と「DER 幅」の行 | 要約の値 | 一致 | 平均は run の平均 DER（小数 1 桁）の算術平均を小数 2 桁に四捨五入した値、幅は最大 − 最小。値は test が各 log の `mean DER` の行から 10 の -6 乗の整数で出した物と同じ（pseudo） | 陽性 | `baseline_summary_records_der_mean_and_spread_of_runs` |
| T-RB-7-022 | fixture：3 つの run の平均 DER が 13.3、13.1、13.5 | 平均と幅の関数の手計算 | 13.30 と 0.4 | 式：`(13.3 + 13.1 + 13.5) / 3` = 13.30、`13.5 − 13.1` = 0.4（derived、手計算） | 陽性 | `mean_and_spread_of_13_3_13_1_13_5_are_13_30_and_0_4` |
| T-RB-7-023 | fixture `rb07_summary_without_der_spread.md`：「DER 平均」の行だけ在る節 | 幅の欠け | `Err` | `Err` で「DER 幅」を名指し（implicit） | 陰性 | `summary_without_der_spread_is_rejected` |

### 4.9 RB-8（wire の録りと要約）

test：`native/hayamimi-eval/tests/rb08.rs`。
入力：`<base>/wire/`（`hayamimi-eval record` の出力）、`docs/results/baseline_python.md`。
Python が出す event の型は 13 種で、`model_load`、`warning`、`error`、`refine`、`translation`、`session_summary`、`session_start`、`session_reset`、`recluster`、`ready`、`partial`、`model_fallback`、`final` である（`grep -ohE '"type": *"[a-z_]+"' scripts/*.py \| sort -u \| wc -l` = 13、2026-10-13）。
route は 8 本（`/events`、`/dashboard`、`/transcript`、`/replacements`、`/itn_overrides`、`/config`、`/reset`、`/`）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RB-8-001 | `<base>/wire/` の録れた型 | 陽性対照の型 | 在る | `ready` と `final` の件数がそれぞれ 1 以上（specified、RB-8 の陽性対照） | 陽性 | `ready_and_final_are_recorded` |
| T-RB-8-002 | 録れた型の集合 | 型の範囲 | 13 種の内側 | 録れた型の全部が上の 13 種に含まれる（derived、`grep` の出力） | 陽性 | `recorded_types_are_among_the_13_known_types` |
| T-RB-8-003 | fixture：型が `bogus` の event を 1 件含む録り | 未知の型 | `Err` | `Err` で `bogus` を名指し（implicit） | 陰性 | `unknown_event_type_is_rejected` |
| T-RB-8-004 | 録れた型ごとの件数 | 件数 | 1 以上 | 録れた型ごとに JSON が 1 件以上で、全部が JSON として読める（specified、RB-8） | 陽性 | `every_recorded_type_has_a_parsable_json` |
| T-RB-8-005 | `baseline_python.md` の型の一覧と録れた型の集合 | 一覧の一致 | 集合が等しい | 両方向に差が 0（specified、RB-8） | 陽性 | `baseline_type_list_equals_recorded_types` |
| T-RB-8-006 | fixture `rb08_list_missing_one.md`：録れた型のうち 1 つが一覧に無い | 一覧の欠け | `Err` | `Err` でその型を名指し（specified、RB-8） | 陰性 | `type_missing_from_baseline_list_is_named` |
| T-RB-8-007 | fixture `rb08_list_with_extra.md`：録れていない型を一覧に足した物 | 一覧の余り | `Err` | `Err` でその型を名指し（derived） | 陰性 | `extra_type_in_baseline_list_is_named` |
| T-RB-8-008 | 録れた route の一覧 | route の数 | 8 本 | 上の 8 本のそれぞれに status と応答の key の集合が在る（specified、RB-8） | 陽性 | `eight_routes_are_recorded_with_status_and_keys` |
| T-RB-8-009 | fixture：route が 7 本の録り | route の数の下側 | `Err` | `Err` で欠けた route を名指し（derived） | 境界、陰性 | `seven_routes_are_rejected` |
| T-RB-8-010 | 鍵 `dGhlIHNhbXBsZSBub25jZQ==` | handshake の accept の計算 | 標準の値 | test の accept の関数が `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=` を返す（specified、RFC 6455 の 1.3 の例） | 陽性 | `accept_key_matches_the_rfc_6455_example` |
| T-RB-8-011 | 録れた `/ingest` の handshake の要求の鍵と応答の accept | 録りの整合 | 一致 | status が 101 で、応答の accept = 010 の関数が録れた鍵から出した値（specified、RFC 6455） | 陽性 | `recorded_ingest_handshake_has_a_valid_accept` |
| T-RB-8-012 | fixture：accept の末尾を 1 文字変えた録り | accept の食い違い | `Err` | `Err`（derived） | 陰性 | `wrong_accept_in_recorded_handshake_is_rejected` |
| T-RB-8-013 | `baseline_python.md` | 要約の中身 | 節と項目が在る | 見出しに `RB-3`、`RB-4`、`RB-5`、`RB-6`、`RB-7` を含む節が 5 つと、`測った機械：`、`Python の版：`、`package の版：`、`日時：` で始まる行が在る。package の版の行に sacrebleu 2.6.0、jiwer 4.0.0、simpleder 0.0.5、OpenCC 1.4.2、pyarrow 25.0.1、sherpa-onnx 1.13.6 を含む（specified、RB-8 と RB-1） | 陽性 | `baseline_summary_has_sections_and_environment_lines` |
| T-RB-8-014 | fixture `rb08_summary_without_rb06.md`：RB-6 の節が無い要約 | 節の欠け | `Err` | `Err` で RB-6 を名指し（specified、RB-8） | 陰性 | `summary_missing_a_section_is_named` |
| T-RB-8-015 | fixture `rb08_summary_without_date.md`：`日時：` の行が無い要約 | 項目の欠け | `Err` | `Err` で日時を名指し（specified、RB-8） | 陰性 | `summary_missing_the_date_line_is_named` |
| T-RB-8-016 | 偽の server（test が bind した port で、SSE の `ready` と `final` を 1 件ずつ返す）へ `record` の部分を向ける | 録りの機構 | 型ごとの件数 | 録れた型の集合が {ready, final}、件数が 1 ずつ（specified、RB-8 の陽性対照を hermetic に確かめる） | 陽性 | `record_against_fake_server_captures_each_event_type` |
| T-RB-8-017 | 閉じた port（bind して port を読み、listener を落とした port）へ `record` を向ける | server を止めた run | 接続の `Err` | 接続の error の型で返る（specified、RB-8 の陰性対照） | 陰性 | `record_against_closed_port_is_a_connect_error` |
| T-RB-8-018 | `<base>/wire/` が無い | 結果の file の欠け | `Err` | `Err` で path 名指し（implicit） | 陰性 | `missing_wire_directory_names_its_path` |

### 4.10 RP の case 行の読み方

RP の test は、実走する物（GPU、Windows、llama-server、長い run を要る物）と、結果の file か fixture だけを読む物に分かれる。
実走する物は `#[ignore]` を付け、表の「閾値」の列に `実走` と書く。実走の command は `rust_port.ja.md` の 6 の command 列のとおりで、case 行はその判定の部分を `check_*` 関数に切り出した形で書く。
判定の `check_*` 関数は fixture で陽性と陰性と境界を確かめ、実走の test は同じ関数に実物の結果を渡す。
この分け方なら、GPU の無い機械でも判定の道具が働くことを `cargo test --release --test rpNN` で確かめられる。

設計が陰性に挙げた「壊した build」（句読点を外した build、ITN を外した build、CPU に落ちる build など）は、壊す口（option 名や feature 名）が設計に無い。
case 行では、その run の結果を表す fixture を `check_*` に渡して `Err` を確かめる形を陰性とする。実走で壊した build を走らせる行は、口を実装の席が置いた後に使える物として行に書き、口の名前は書かない。
fixture は `native/<crate>/tests/fixtures/rpNN_*` に置く。fixture の数は、行の式が決めた値を使い、本物の実測のふりをしない。

### 4.11 RP-1（認識の誤り率）

test：`native/hayamimi-eval/tests/rp01.rs`。
`check_scorecard(python, rust)` は、言語ごとの `mean err` と LID の正解の数を比べる。
fixture の python は ja 0.038、en 0.023、zh 0.066、ko 0.081、yue 0.061、LID 正解は ja 15、en 15、zh 12、ko 12、yue 12（`docs/results/scorecard.md` の表と同じ）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-1-001 | rust の値が python と全言語で同じ、LID も同じ | 等号 | `Ok` | `Ok`。「以下」「以上」の等号側（specified、RP-1） | 境界、陽性 | `equal_to_python_passes` |
| T-RP-1-002 | rust の ja が 0.039、ほかは同じ | 上側へ最小の桁 0.001 | `Err` | `Err` で ja を名指し（specified、RP-1。ja は CER） | 境界、陰性 | `ja_one_thousandth_worse_is_rejected` |
| T-RP-1-003 | rust の en が 0.024、ほかは同じ | en は WER | `Err` | `Err` で en を名指し（specified、RP-1。言語ごとに独立に比べる） | 陰性 | `en_one_thousandth_worse_is_rejected` |
| T-RP-1-004 | rust が全言語で 0.001 ずつ良い | 全言語が下側 | `Ok` | `Ok`（specified、RP-1） | 陽性 | `all_languages_better_pass` |
| T-RP-1-005 | rust の LID 正解が ja 14（python は 15） | LID の数の下側 | `Err` | `Err` で ja の LID を名指し（specified、RP-1 の「Python 以上」） | 境界、陰性 | `one_fewer_correct_lid_is_rejected` |
| T-RP-1-006 | rust の表に yue の行が無い | 言語の欠け | `Err` | `Err` で yue を名指し（implicit） | 陰性 | `missing_language_is_rejected` |
| T-RP-1-007 | `<base>` の Python の値と `docs/results/scorecard.md` | 陽性対照 | 差 ≤ 0.005 | 言語ごとに差 ≤ 0.005（specified、RP-1 の陽性対照）。`<base>` の Python の値は RB-3 の出力（`scorecard_run1.md`、`scorecard_run2.md`）の言語ごとの平均（設計 RP-1） | 陽性 | `python_baseline_matches_scorecard_md_within_0_005` |
| T-RP-1-008 | 参照 `Hello, World.`、hyp `hello world`（en） | WER の正規化 | 0 | WER = 0。式：小文字化、NFKC、句読点を空白、空白をまとめる（derived、`eval_accuracy.py` の `normalize_en`） | 陽性 | `wer_ignores_case_and_punctuation` |
| T-RP-1-009 | 参照 `the cat sat on the mat`、hyp `the cat sat on a mat` | WER の置換 1 語 | 1/6 | WER = 0.1667（小数 4 桁）。式：距離 1 ÷ 語数 6（pseudo、awk の `1/6`） | 陽性 | `wer_one_substitution_in_six_words` |
| T-RP-1-010 | 参照が空白と句読点だけ、hyp `x` | 参照の語が 0 個 | 0 | WER = 0。0 で割らない（derived、`wer_en` が語数 0 で 0.0） | 境界 | `wer_of_empty_reference_is_0` |
| T-RP-1-011 | 参照 `學習`、hyp `学习`（yue） | 繁体と簡体 | 0 | CER = 0（derived、yue は OpenCC の t2s を両側に掛けて採点する。Rust の t2s の道具は設計に無い） | 陽性 | `yue_cer_normalises_traditional_to_simplified` |
| T-RP-1-012 | `<ev> scorecard --engine hayamimi --sets …` の結果 `rp01.json` | 実走 | 実走 | 言語ごとの平均の誤り率が Python 以下、LID も以上（specified、RP-1）。`rp01.json` に `model.fallback.*` の event が 1 件でも在れば、既定の経路を測っていないので `Err` でその event を名指しする（specified、設計 8 の「07:00 の計画」の R3 の着手の 4） | 陽性 | `hayamimi_scorecard_not_worse_than_python` |
| T-RP-1-013 | 振り分けで ja の clip を SenseVoice の tier へ送る build の `rp01.json`（口は実装の席が置く。振り分けの出力を ja の clip だけ SenseVoice の tier に差し替える。ja の clip は 15 本、LID は ja の clip ごとに数える） | 実走の陰性。ja の clip の tier だけを替え、ほかの言語の経路は T-RP-1-012 の run と同じ | 実走 | `check_scorecard` が `Err` で ja を名指しする。ja の平均の CER を小数 3 桁へ丸めた値が 0.038 を超え、ja の LID の正解の数が 15 を割る（specified、RP-1 の陰性対照）。参考値は R3b の検証の席の申告で、clip ごとに session を戻すと ja_02 が zh と読まれた時の ja の誤り率 75147（百万分率、0.075）と LID 14（転記。この build で打ち直すまで NOT VERIFIED。合否には使わず、`Err` と上の 2 つの不等号で決める） | 陰性（T-RP-1-012 の同じ組が通ることが対。ja の名指しは、ja 以外の 4 言語の平均が T-RP-1-012 の run と同じで `Err` に入らないことで確かめる）、陽性（T-RP-1-005 が LID 14 を、T-RP-1-002 が ja の 0.039 を、同じ `check_scorecard` で落とす） | `sensevoice_routed_ja_run_is_worse_than_python_in_ja` |

### 4.12 RP-2（ja の golden 8 本）

test：`native/hayamimi-asr/tests/rp02.rs`。
`check_golden(recorded, rust)` は、clip ごとに Rust の CER が `golden.json` の `cer_finals_vs_reference` 以下であることを確かめる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-2-001 | `tests/golden/ja/golden.json` | 記録の読み取り | 8 clip | `clips` が 8 つで、ja_014 の `cer_finals_vs_reference` が 0.0222（derived。`jq '.clips[0].cer_finals_vs_reference'` の値、2026-10-13 に読んだ） | 陽性 | `golden_json_has_eight_clips_and_reads_ja_014_cer_0_0222` |
| T-RP-2-002 | rust の CER が 8 clip とも記録と同じ | 等号 | `Ok` | `Ok`（specified、RP-2 の「以下」） | 境界、陽性 | `equal_cer_passes` |
| T-RP-2-003 | ja_014 だけ記録 + 0.0001（記録は小数 4 桁） | 上側へ最小の桁 | `Err` | `Err` で ja_014 を名指し（specified、RP-2） | 境界、陰性 | `cer_one_ten_thousandth_above_record_is_rejected` |
| T-RP-2-004 | fixture：ja_085 と ja_090 が記録より悪い | ITN を外した run | `Err` | `Err` で ja_085 と ja_090 の 2 つを名指し（specified、RP-2 の陰性対照） | 陰性 | `itn_off_fixture_names_ja_085_and_ja_090` |
| T-RP-2-005 | fixture：clip が 7 つの結果 | 数の下側 | `Err` | `Err`（derived、8 本でない） | 境界、陰性 | `seven_clips_are_rejected` |
| T-RP-2-006 | `tests/golden/ja/*.wav` と `golden.json` の `sha256` | 同じ clip を読むこと | 一致 | 8 本とも test が出した sha256 = 記録（derived、`sha256` の関数は RB-2 の 007 で確かめた物） | 陽性 | `golden_wavs_have_the_recorded_sha256` |
| T-RP-2-007 | rust の結果から書いた `tests/golden/ja/golden_gpu.json` | 実走の出力 | 実走 | file が在り、`clips` が 8 つで、各 clip が `finals` を持つ（specified、RP-2 の「別に記録する」） | 陽性 | `golden_gpu_json_is_written_with_eight_clips` |
| T-RP-2-008 | `<hy> --wav tests/golden/ja/<clip> --no-realtime --mode single --lang ja` の 8 本 | 実走 | 実走 | 各 clip の CER が記録以下（specified、RP-2） | 陽性 | `hayamimi_gpu_golden_cer_not_worse_than_recorded` |
| T-RP-2-009 | ITN を外した設定の 8 本（口は実装の席が置く） | 実走の陰性 | 実走 | ja_085 と ja_090 が記録を超えて `check_golden` が `Err`（specified、RP-2 の陰性対照） | 陰性 | `itn_off_run_fails_ja_085_and_ja_090` |

### 4.13 RP-3（主指標 `L`）

test：`native/hayamimi-eval/tests/rp03.rs`。
`pair_translation(final_text, events)` は、`translation` event の `src_sha256` が `final` の文字列の sha256 と一致する最初の物の時刻を `t_tr` にする。
`check_latency(python, rust)` は `L` の p50 と p95、発話ごとの中央値の比べ方を確かめる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-3-001 | `final` の文字列 `abc`、時刻 100 の `translation` が `src_sha256` = `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` | 原文が final と一致する訳 | 対応づく | `t_tr` = 100（specified、3.1。sha256 の値は FIPS 180-2 の標準の値） | 陽性 | `matching_src_sha256_pairs_the_translation` |
| T-RP-3-002 | `final` の文字列 `abc`、`translation` の `src_sha256` が `ab` の sha256 | 断片の訳 | 対応づかない | `t_tr` が無く、取れない発話に数える（specified、3.1） | 陰性 | `translation_of_a_partial_text_is_not_paired` |
| T-RP-3-003 | `final` の `abc`、時刻 100 の断片の訳、時刻 200 の一致する訳 | 断片の訳が先に来る | 200 | `t_tr` = 200（specified、3.1 の「初めて受けた」物は一致する訳に限る） | 境界 | `first_translation_matching_the_final_wins` |
| T-RP-3-004 | python の p50 = 1000 ms、p95 = 2000 ms、rust が同じ | 等号 | `Ok` | `Ok`（specified、RP-3 の「以下」） | 境界、陽性 | `equal_percentiles_pass` |
| T-RP-3-005 | rust の p50 = 1001 ms | p50 の上側へ 1 ms | `Err` | `Err` で p50 を名指し（specified、RP-3） | 境界、陰性 | `p50_one_millisecond_slower_is_rejected` |
| T-RP-3-006 | rust の p95 = 2001 ms | p95 の上側へ 1 ms | `Err` | `Err` で p95 を名指し（specified、RP-3） | 境界、陰性 | `p95_one_millisecond_slower_is_rejected` |
| T-RP-3-007 | fixture：`L` の比較集合が 19 発話で、そのうち 17 で rust の中央値が python 以下 | 発話ごとの比較の境界 | `Ok` | `Ok`。17 = 19 × 20 / 23 の切り上げ（specified、RP-3。19 発話は実物の数で、下の注） | 境界、陽性 | `seventeen_of_19_comparison_utterances_faster_passes` |
| T-RP-3-008 | 同じ fixture で 16 | 境界の下側 | `Err` | `Err`（specified、RP-3） | 境界、陰性 | `sixteen_of_19_comparison_utterances_faster_is_rejected` |
| T-RP-3-009 | en→ja（lat-en）の rust の p95 = 2000 ms | 仮の上限の等号 | `Ok` | `Ok`（specified、RP-3。2000 は仮の値。lat-en の Python の基準が無いので R8e が測り、超えたら評議へ戻す） | 境界、陽性 | `en_to_ja_p95_of_2000_ms_passes` |
| T-RP-3-010 | 同じ p95 = 2001 ms | 仮の上限の外側 | `Err` | `Err`（specified、RP-3） | 境界、陰性 | `en_to_ja_p95_of_2001_ms_is_rejected` |
| T-RP-3-011 | 結果の表（`L`、`L_asr`、`L_tr`、`L_tr` が負の発話の数） | 出力の列 | 全部在る | 列が 4 つ在る（specified、RP-3） | 陽性 | `report_has_separate_columns_and_the_negative_l_tr_count` |
| T-RP-3-012 | fixture：`L_tr` の列が無い結果の表 | 列の欠け | `Err` | `Err` で `L_tr` を名指し（specified、RP-3） | 陰性 | `report_without_l_tr_column_is_rejected` |
| T-RP-3-013 | R0 の python の run（`<base>/lat_ja_0.35_*.jsonl`） | 陽性対照 | 標本 | 同じ関数が p50 と p95 を出し、各 run で 有効 + final なし + 終端不正 = 23 × 5、有効な標本の全件で `L_asr − L_dec ≥ min_silence`、`L` の標本の数 = 有効な標本のうち訳が取れた数（specified、RP-3 の陽性対照。分割は RB-5 の 003 から 005 の式で読む） | 陽性 | `python_run_yields_p50_and_p95_and_splits_its_frame_into_valid_no_final_and_bad_end` |
| T-RP-3-014 | fixture：rust の `L` が python の全発話より 600 ms 大きい | 訳の依頼に 600 ms の待ちを入れた偽の llama-server | `Err` | `Err` で p50 と p95 の両方（specified、RP-3 の陰性対照） | 陰性 | `six_hundred_ms_slower_translation_is_rejected` |
| T-RP-3-015 | fixture：全 `translation` の `src_sha256` が final と一致しない run | 断片の訳だけを返す偽の中継 | `Err` | `Err`（取れない発話が全部で、`t_tr` が 0 件）（specified、RP-3 の陰性対照） | 陰性 | `relay_returning_only_fragment_translations_is_an_error` |
| T-RP-3-016 | python の run と rust の run の起動引数 `--min-silence` が 0.35 と 0.30 | 同じ値を渡すこと | `Err` | `Err` で 2 つの値を名指し（specified、RP-3 の「R0 で決めた値を両方に渡す」） | 陰性 | `different_min_silence_between_runs_is_rejected` |
| T-RP-3-017 | `<ev> stream --wav lat-ja.wav --repeat 5` を python と rust に | 実走 | 実走 | ja→en で rust の p50 と p95 が python 以下で、比較集合の発話のうち「発話の数 × 20 / 23 の切り上げ」以上で rust の 5 回の中央値が python 以下（specified、RP-3。R0 の 2 回では 19 発話のうち 17。閾値は 018 の関数で出す） | 陽性 | `rust_l_beats_python_on_lat_ja` |
| T-RP-3-018 | 比較集合の発話の数 n = 19、20、23、1 | 対の比較の閾値の式 | 17、18、20、1 | 閾値 = n × 20 / 23 の切り上げ。式：`(n × 20 + 22) / 23`（整数の割り算）。手計算：380 / 23 = 16.5 → 17、400 / 23 = 17.4 → 18、460 / 23 = 20 → 20、20 / 23 = 0.87 → 1（derived。RP-4 も同じ関数を使う） | 境界、陽性 | `comparison_threshold_is_n_times_20_over_23_rounded_up` |
| T-RP-3-019 | 比較集合の発話の数 n = 0 | 空の集合 | `Err` | `Err`。比較集合が空の run を「全件以下」と読まない（implicit） | 境界、陰性 | `empty_comparison_set_is_an_error_not_a_pass` |
| T-RP-3-020 | fixture dir `rp03_python_runs/`：python の 2 run。発話 5 は有効だが訳が無い、発話 10 は final なし、発話 18 は終端不正だけ、発話 19 は final なし、発話 2 は run 1 だけ有効で訳が取れる、残りの 18 発話は 2 回とも有効で訳が取れる。rust の run は全 23 発話に有効で訳の取れた標本を持つ | 比較集合は Python の run だけで決まる | 18 発話 | `L` の比較集合は 18 発話で、2、5、10、18、19 を含まない。rust の run に標本が在っても入らない（specified、3.1 の「比較集合は Python の run だけで決め、Rust の結果では動かさない」） | 陽性、陰性 | `comparison_set_is_fixed_by_python_runs_and_ignores_the_rust_run` |
| T-RP-3-021 | R0 の python の 2 run（`<base>/lat_ja_0.35_1.jsonl`、`lat_ja_0.35_2.jsonl`） | `L` の比較集合（実物） | 19 発話 | 発話 1〜4、6〜9、11〜17、20〜23 の 19 発話で、5、10、18、19 を含まない。19 は別の式（jq、下の注）が出した値で、test が定数に持つ。R0 で Python を測り直したら、注の値と同じ手番で直す（pseudo） | 陽性 | `l_comparison_set_of_the_python_runs_has_19_utterances_without_5_10_18_19` |

比較集合の発話の数（021 と RP-4 の 009 の定数の出所）を、R0 の Python の 2 run に打って数えた。
`testdata/baseline/` で、有効の判定は `L_asr − L_dec ≥ 350 ms`、組の重複は `unique` で除き、2 run に在る発話だけを残す。

```
$ jq -n '[inputs | select(.t_final_us != null and ((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) >= 350 and .t_tr_us != null) | {f: input_filename, u: .utt}] | unique | group_by(.u) | map(select(length == 2)) | length' lat_ja_0.35_1.jsonl lat_ja_0.35_2.jsonl
19
$ jq -n '[inputs | select(.t_final_us != null and ((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) >= 350) | {f: input_filename, u: .utt}] | unique | group_by(.u) | map(select(length == 2)) | length' lat_ja_0.35_1.jsonl lat_ja_0.35_2.jsonl
20
```

1 本目が `L` の比較集合、2 本目が `L_asr` の比較集合（RP-4 の 009）である。
2 本の差は発話 5 で、有効な標本は 2 run で 10 あり、訳が取れた標本は 0 である。

陽性対照：有効の判定を外して `select(.t_final_us != null)` だけで同じ式を打つと 21 で、`L_asr` の比較集合の 20 と違う。
差は発話 18 で、`final` は在るが、有効な標本は 2 run で 0 である。
陰性対照：`select(.t_tr_us != null)` だけで打つと 19 で、`L` の比較集合と同じになる。この 2 run では、有効の判定は `L` の集合の数を動かさず、`L_asr` の集合の数だけを動かす。

### 4.14 RP-4（認識の遅れ `L_asr`）

test：`native/hayamimi-eval/tests/rp04.rs`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-4-001 | `L_asr` の python の p50 = 800 ms、p95 = 1500 ms、rust が同じ | 等号 | `Ok` | `Ok`（specified、RP-4） | 境界、陽性 | `equal_l_asr_percentiles_pass` |
| T-RP-4-002 | rust の p50 = 801 ms | 上側へ 1 ms | `Err` | `Err` で p50 を名指し（specified、RP-4） | 境界、陰性 | `l_asr_p50_one_millisecond_slower_is_rejected` |
| T-RP-4-003 | rust の p95 = 1501 ms | 上側へ 1 ms | `Err` | `Err` で p95 を名指し（specified、RP-4） | 境界、陰性 | `l_asr_p95_one_millisecond_slower_is_rejected` |
| T-RP-4-004 | fixture：`L_asr` の比較集合が 20 発話で、そのうち 18 / 17 で rust の 5 回の中央値が python 以下 | 発話ごとの比較 | 18 は `Ok`、17 は `Err` | 閾値は RP-3 の 018 の関数で、20 × 20 / 23 の切り上げ = 18（specified、RP-4。20 発話は実物の数で、RP-3 の注） | 境界 | `eighteen_of_20_comparison_utterances_passes_and_seventeen_is_rejected` |
| T-RP-4-005 | rust の `L_dec` の p50 が python の 2 倍だが `L_asr` は以下 | `L_dec` は判定に入らない | `Ok` | `Ok`。`L_dec` は記録だけに回る（specified、RP-4） | 境界 | `l_dec_is_recorded_but_does_not_decide` |
| T-RP-4-006 | python の run の全 `final` の `latency_ms` を、WS の `final` event と Python の stdout の log から別々に出した本数と平均 | 陽性対照 | 本数が同じで差 0.5 ms 以内 | 本数が同じで、平均の差が 0.5 ms 以内。RB-5 の 019 から 021 と同じ関数を呼ぶ。発話の最後の `final` だけの平均とは母集団が違うので比べない（specified、RP-4） | 陽性 | `python_final_latency_from_ws_and_log_agree_in_count_and_within_0_5_ms` |
| T-RP-4-007 | provider が `cpu` の rust の run の結果 | 記録だけ | 記録が在る | 同じ式の値が記録の file に在り、`provider` の項目が `cpu`。値の大小では落とさない（specified、RP-4 の陰性欄の「記録する」） | 陰性 | `cpu_provider_run_is_recorded_not_judged` |
| T-RP-4-008 | `<ev> stream` の python と rust の run | 実走 | 実走 | `L_asr` の p50 と p95 が python 以下で、比較集合の発話のうち「発話の数 × 20 / 23 の切り上げ」以上で rust の 5 回の中央値が python 以下（specified、RP-4。R0 の 2 回では 20 発話のうち 18。閾値は RP-3 の 018 の関数） | 陽性 | `rust_l_asr_beats_python_on_lat_ja` |
| T-RP-4-009 | R0 の python の 2 run（`<base>/lat_ja_0.35_1.jsonl`、`lat_ja_0.35_2.jsonl`） | `L_asr` の比較集合（実物） | 20 発話 | 発話 1〜9、11〜17、20〜23 の 20 発話で、10、18、19 を含まない。発話 5 は入る（訳は無いが有効な標本が在る）。20 は別の式（jq、RP-3 の注）が出した値で、test が定数に持つ。有効の判定を外して数えた 21 と違うことを、test の中の判定なしの数え方で確かめる（pseudo） | 陽性、陰性 | `l_asr_comparison_set_of_the_python_runs_has_20_utterances_with_5_and_without_10_18_19` |

### 4.15 RP-5（速報の字幕と速報の訳）

test：`native/hayamimi-eval/tests/rp05.rs`。
`t_partial − 発話の始まりの sample を送った時刻` を `first_partial_delay` と呼ぶ。発話の始まりの sample の位置は R0 の manifest（RB-2）が持つ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-5-001 | `first_partial_delay` の p95 が python と rust で同じ | 等号 | `Ok` | `Ok`（specified、RP-5） | 境界、陽性 | `equal_first_partial_p95_passes` |
| T-RP-5-002 | rust の p95 が python より 1 ms 大きい | 上側へ 1 ms | `Err` | `Err`（specified、RP-5） | 境界、陰性 | `first_partial_p95_one_millisecond_slower_is_rejected` |
| T-RP-5-003 | 速報の訳の時刻 400、`t_tr` が 500 の発話 | 速報の訳が先 | `Ok` | `Ok`（specified、RP-5 の「`t_tr` より前」） | 陽性 | `partial_translation_before_t_tr_passes` |
| T-RP-5-004 | 速報の訳の時刻 500、`t_tr` が 500 | 同時 | `Err` | `Err`。より前でなければならない（specified、RP-5） | 境界、陰性 | `partial_translation_at_the_same_time_as_t_tr_is_rejected` |
| T-RP-5-005 | 速報の訳の時刻 501、`t_tr` が 500 | 遅い | `Err` | `Err`（specified、RP-5） | 境界、陰性 | `partial_translation_after_t_tr_is_rejected` |
| T-RP-5-006 | 速報の訳が 1 つも出ていない発話 | 判定の対象外 | `Ok` | `Ok`。速報の訳が出た発話だけが対象（specified、RP-5） | 境界 | `utterance_without_partial_translation_is_not_judged` |
| T-RP-5-007 | python の run | 陽性対照 | 時刻が取れる | `final` を持つ全発話で最初の `partial` の時刻が取れ、その `first_partial_delay` は正である。`final` の無い発話（3.1 の「final なし」）は、`partial` が在っても無くても p95 に入れず、対象外である。記録した run は 115 行中 12 行が、`final` も `partial` も無い発話である。`final` を持つ行と持たない行がどちらも在ることも確かめる（specified、RP-5 の陽性対照） | 陽性 | `python_run_yields_first_partial_times` |
| T-RP-5-008 | 速報の訳を有効にした run の fixture と、切った run の fixture | 数える道具の確かめ | 切った run は 0、有効の run は 1 以上 | 速報の訳の数の関数が 0 と 1 以上を返す（specified、RP-5 の陰性欄） | 陰性 | `partial_translation_counter_reads_zero_when_disabled` |
| T-RP-5-009 | `<ev> stream` の python と rust の run | 実走 | 実走 | `first_partial_delay` の p95 が python 以下、速報の訳が出た発話で時刻が `t_tr` より前（specified、RP-5） | 陽性 | `rust_partials_arrive_no_later_than_python` |
| T-RP-5-010 | fixture `native/hayamimi-eval/tests/fixtures/rp05_partial_without_final.jsonl`：6 発話。`final` を持つ 3 発話の `first_partial_delay` が 200、300、400 ms。`partial` だけで `final` の無い 1 発話の最初の `partial` の遅れが 9000 ms。`final` も `partial` も無い 2 発話。この fixture を python の run としても rust の run としても `first_partial_delay` の p95 の関数に渡す | `partial` だけで `final` の無い発話が p95 に入るか | p95 = 400 ms | python の run でも rust の run でも p95 = 400。式：`final` を持つ 3 発話だけで n = 3、`(95 × 3 + 99) / 100` = 3 番目 = 400（specified、RP-5 の「`final` の無い発話は p95 に入れず」と 3.1 の nearest-rank）（pseudo、awk の `int((95*3+99)/100)`）。`partial` だけの発話を入れると n = 4、`(95 × 4 + 99) / 100` = 4 番目 = 9000 になり落ちる | 陽性、陰性 | `partial_only_utterance_is_left_out_of_the_first_partial_p95` |
| T-RP-5-011 | 010 の fixture | `partial` だけの発話と、どちらも無い発話を別の数にするか | 1 / 2 / 3 | 結果の file の 3 つの数が、`partial` だけで `final` の無い発話 1、`final` も `partial` も無い発話 2、`final` を持つ発話 3 で、和が 6（specified、RP-5 の「結果の file に別々に書く」）（pseudo、fixture を手で数えた値）。合算すると 3 と 3 になり、1 と 2 の取り違えも落ちる | 陽性、陰性 | `partial_only_and_neither_utterances_are_counted_separately` |
| T-RP-5-012 | 010 の fixture から `partial` だけの 1 発話を除いた 5 発話 | 数える道具の確かめ（`partial` だけが 0 件） | 0 / 2 / 3 | `partial` だけの発話の数が 0、どちらも無い発話の数が 2、`final` を持つ発話の数が 3。p95 は 400 のまま（specified、RP-5）。0 は 011 で 1 を返した同じ関数の値で、「数えていない 0」と「数えて 0」を 011 が分ける | 陰性 | `counter_reads_zero_partial_only_utterances_when_there_are_none` |

中継の側の速報の訳は `native/hayamimi-relay/tests/r8d_partial.rs` が測る。
次の 11 本で、順に、次の `id` で訳す、最初の確定文の前は訳さない、新しい原文が古い要求を打ち切る、確定文が来たら打ち切る、2 つの入力が互いを止めない、訳さない言語は飛ばす、外の送り先へ渡さない、設定で切ると 1 本も訳さない（陰性対照）、入れると 1 本訳す（陽性対照）、`session_reset` で番号を忘れる、確定した字形で描いた接頭辞を消す訳は出さず前の訳を残す（`a_translation_in_progress_does_not_take_away_the_committed_prefix`、RP-25 の 3 つ目の条件。守りを外すと落ちる陰性対照を確かめた）、である。
速報の訳を切る設定は `--no-partial-translation` である。

### 4.16 RP-6（推論が GPU で走ったこと）

test：fixture と docs を読む case（012 と 013 を除く全部）は `native/hayamimi-eval/tests/rp06.rs`。実走の 012 と 013 は R2 が作る `native/hayamimi/tests/rp06.rs`。
GPU の memory の読みは `nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits` で、1 行が MiB の整数 1 つである。`--query-compute-apps` は WSL で process の名前が `[Not Found]`、Windows で使用量が `[N/A]` になるので使わない。基準は起動前の 3 回の読みの中央値、増えた量の閾値は 256 MiB、戻った幅の閾値は ± 64 MiB（設計の RP-6、仮の値）。
実行中と終了後 10 秒までは、0.25 秒以下の間隔で止めずに読み続ける。GPU の memory が大きく使われるのは 0.5 秒ほどで、1 秒ごとの読みでは山を取り逃す（WSL の実測で、1 秒ごとの最大は基準 + 131 MiB、0.25 秒ごとでは + 773 MiB）。
012 と 013 は同じ GPU の `memory.used` を読むので、同時に走らせない（test の中で 1 つの Mutex を取ってから run を起こす）。並べて走らせると、013 の読みが 012 の山を拾って落ちる。ほかの GPU の process も止めた機械で打つ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-6-001 | fixture：起動前 1198、1198、1199、実行中の最大 1957、終了後 1198（MiB。Windows の実測の形） | 増えて戻る | `Ok` | `check_memory_rise` が `Ok`。式：基準 1198、1957 − 1198 = 759 ≥ 256、\|1198 − 1198\| = 0 ≤ 64（specified、RP-6） | 陽性 | `memory_that_rises_during_the_run_and_returns_passes` |
| T-RP-6-002 | fixture：基準 1198、実行中の最大 1454 | 増えた量の等号 | `Ok` | `Ok`。式：1454 − 1198 = 256 ≥ 256（specified、RP-6。256 は仮の値） | 境界、陽性 | `rise_of_exactly_256_mib_passes` |
| T-RP-6-003 | fixture：基準 1198、実行中の最大 1453 | 増えた量の外側 | `Err` | `Err` で増えた量 255 を名指し（specified、RP-6） | 境界、陰性 | `rise_of_255_mib_is_rejected` |
| T-RP-6-004 | fixture：基準 1198、実行中の最大 1957、終了後 10 秒の読みが全部 1263 以上 | 戻らない | `Err` | `Err` で終了後の読みを名指し。式：1263 − 1198 = 65 > 64（specified、RP-6 の「基準 ± 64 MiB へ戻る」） | 境界、陰性 | `memory_that_does_not_return_after_exit_is_rejected` |
| T-RP-6-005 | fixture：`memory.used` の読みが起動前に 2 回しか無い、または `[N/A]` の行を含む | 読みの欠け | `Err` | `Err`。3 回そろわない基準と数でない読みを 0 と読まない（implicit） | 境界、陰性 | `missing_or_non_numeric_memory_reads_are_rejected` |
| T-RP-6-006 | fixture：3.5 の表で GPU とした段（言語判定、句読点）の `gpu.session.created` の全件で実際の provider が `cuda`、CPU とした段（VAD、認識）の session が `cpu` | provider | `Ok` | `Ok`（specified、RP-6） | 陽性 | `all_sessions_on_cuda_pass` |
| T-RP-6-007 | fixture：006 の GPU とした段の 1 件だけ実際の provider が `cpu` | 1 件の例外 | `Err` | `Err` でその model を名指し（specified、RP-6） | 陰性 | `one_cpu_session_names_its_model` |
| T-RP-6-008 | fixture：`gpu.session.created` が 0 件 | 空 | `Err` | `Err`。0 件を「全件の置き場が表と同じ」と読まない（implicit） | 境界、陰性 | `no_session_events_is_an_error` |
| T-RP-6-009 | fixture：node の表（`gpu_stages.md` の形）の `punct_bert.onnx` の行が CUDA の node 453、CPU の node 77。`gpu.session.placed` の `punct_bert.onnx`（3.5 の表で GPU の句読点）の CPU の node が 77 | R1 の値の等号 | `Ok` | `Ok`。式：77 ≤ 77 が真（specified、RP-6 の「R1 の値以下」。77 と 453 は `gpu_stages.md` の node の表の `punct_bert.onnx` の行の値） | 境界、陽性 | `gpu_model_at_the_recorded_cpu_node_count_passes` |
| T-RP-6-010 | fixture：009 と同じで、`gpu.session.placed` の `punct_bert.onnx` の CPU の node が 78 | R1 の値の外側（1 つ上） | `Err` | `Err` で `punct_bert.onnx` と 78 と 77 を名指し。式：78 ≤ 77 が偽（specified、RP-6） | 境界、陰性 | `gpu_model_one_node_over_the_recorded_count_is_rejected` |
| T-RP-6-011 | fixture：3.5 の表に載らない model（`unlisted.onnx`）の `gpu.session.placed` の CPU の node が 5。node の表に行は無い | 判定の対象外 | `Ok` | `Ok`。記録だけ（specified、RP-6 の「表に載らない model は記録だけ」） | 境界 | `cpu_nodes_of_unlisted_models_are_recorded_only` |
| T-RP-6-012 | 実走：`--wav tests/golden/ja/ja_014.wav --no-realtime --mode single --lang ja --gpu-diagnose` と、0.25 秒以下の間隔で止めずに読み続ける `nvidia-smi` の読み（013 と同時に走らせない） | 実走 | 実走 | 実行中の `memory.used` の最大が基準 + 256 MiB 以上で、終了後 10 秒以内に基準 ± 64 MiB へ戻る。`gpu.session.created` の provider が 3.5 の表の置き場と全件で同じ。`gpu.session.placed` の `punct_bert.onnx` の CPU の node が、node の表の同じ model の値（いまは 77）以下（specified、RP-6） | 陽性 | `wav_run_uses_cuda_and_appears_in_compute_apps` |
| T-RP-6-013 | 実走：`CUDA_VISIBLE_DEVICES` を空にして起こした run | GPU を隠す | 実走 | 実行中の `memory.used` の最大が基準 + 64 MiB を超えず、`gpu.device.unavailable` が 1 件（specified、RP-6 の陰性欄） | 陰性 | `hidden_gpu_run_shows_no_compute_app_and_one_unavailable_event` |
| T-RP-6-014 | fixture：009 と同じで、`gpu.session.placed` の `punct_bert.onnx` の CPU の node が 0 | R1 の値の下端 | `Ok` | `Ok`。式：0 ≤ 77 が真（specified、RP-6） | 境界、陽性 | `gpu_model_with_zero_cpu_nodes_passes` |
| T-RP-6-015 | fixture：node の表に認識の encoder（`encoder-epoch-35-avg-1.onnx`、CUDA の node 3537、CPU の node 1599）の行が在り、3.5 の表ではこの段が CPU。`gpu.session.placed` の同じ model の CPU の node が 1600 | 判定の対象は 3.5 の表が決める（node の表に行が在るだけでは判定しない） | `Ok` | `Ok`。判定の対象は 3.5 の表で GPU とした model だけ（specified、RP-6）。1599 と 3537 は node の表の行の値（derived）。node の表の全行を判定する実装は 1600 ≤ 1599 が偽で落ちる | 境界、陰性 | `cpu_placed_model_is_not_judged_by_the_node_table` |
| T-RP-6-016 | fixture：009 と同じ node の表と 3.5 の表で、`gpu.session.placed` に `punct_bert.onnx` の行が 1 件も無い | 記録の欠け | `Err` | `Err` で `punct_bert.onnx` を名指し。記録が無いことを「CPU の node が 0」と読まない（implicit。008 と同じ型） | 境界、陰性 | `gpu_model_without_a_placed_event_is_rejected` |
| T-RP-6-017 | fixture：3.5 の表で CPU とした段（認識）の session が provider `cuda` で作られた `gpu.session.created` | 逆向きの例外 | `Err` | `Err` でその model を名指し（specified、RP-6 の「CPU とした段は `cpu`」） | 陰性 | `one_cuda_session_in_a_cpu_stage_names_its_model` |
| T-RP-6-018 | fixture：3.5 の表で CPU とした段の model（`encoder-epoch-35-avg-1.onnx`）の `gpu.session.placed` の CPU の node が 5。node の表に行は無い | CPU に置いた model で表に行が無い | `Ok` | `Ok`。記録だけ（specified、RP-6 の「3.5 の表で CPU とした model は記録だけ」） | 境界 | `cpu_placed_model_without_a_node_table_row_is_recorded_only` |
| T-RP-6-019 | fixture：3.5 の表で GPU とした言語判定の model（`tiny-encoder.onnx`）の `gpu.session.placed` の CPU の node が 0。node の表に `tiny-encoder.onnx` の行は無い（句読点の行だけ在る） | GPU に置いた model の行の欠け | `Err` | `Err` で `tiny-encoder.onnx` を名指し。行が無いことを「0 ≤ 値」と読まない（specified、RP-6 の「GPU とした model が node の表に行を持たなければ落ちる」） | 境界、陰性 | `gpu_model_without_a_node_table_row_is_rejected` |
| T-RP-6-020 | 3.5 の表と `gpu_stages.md` の node の表の実物（docs だけを読む。testdata も GPU も要らない。`--ignored` 無し） | 実物の読み取り | 全 model | 3.5 の表で GPU とした model（`tiny-encoder.onnx`、`tiny-decoder.onnx`、`punct_bert.onnx` の 3 つ）の全部が node の表に行を持つ（specified、RP-6 の「GPU とした model が node の表に行を持たなければ落ちる」） | 陽性 | `real_node_table_has_a_row_for_every_gpu_model` |

### 4.17 RP-7（段の間の転送）

test：`native/hayamimi-eval/tests/rp07.rs`。
`nsys profile --stats=true` の統計から、1 発話あたりの host → device と device → host の回数、bytes、時間を p50 と p95 で `docs/results/gpu_stages.md` に置く。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-7-001 | fixture：転送の時間の p95 = 2000 µs、`decode_ms` の p95 = 10 ms | 20% の等号 | `Ok` | `Ok`。式：2000 ÷ 10000 = 20%（specified、RP-7。20% は仮の値で、R7 が認識の置き場が決まった構成で置き直す） | 境界、陽性 | `transfer_p95_at_twenty_percent_of_decode_passes` |
| T-RP-7-002 | fixture：転送の時間の p95 = 2001 µs、同じ `decode_ms` | 20% の外側 | `Err` | `Err`（specified、RP-7） | 境界、陰性 | `transfer_p95_just_over_twenty_percent_is_rejected` |
| T-RP-7-003 | fixture：`gpu_stages.md` の転送の行が 4 項目（回数、bytes、時間、向き）を全部持つ | 項目 | `Ok` | `Ok`（specified、RP-7） | 陽性 | `transfer_row_with_all_fields_passes` |
| T-RP-7-004 | fixture：bytes の項目が無い行 | 項目の欠け | `Err` | `Err` で bytes を名指し（specified、RP-7） | 陰性 | `transfer_row_without_bytes_is_rejected` |
| T-RP-7-005 | fixture：nsys の統計の CSV（`cuda_gpu_mem_time_sum` と `cuda_gpu_mem_size_sum`） | 統計の読み取り | 読める | host → device と device → host の回数と bytes と時間が読める。式：fixture の行を手で足した値（derived） | 陽性 | `nsys_memory_stats_are_parsed_into_both_directions` |
| T-RP-7-006 | fixture：統計に encoder の kernel の行を含む | 同じ式の陽性対照 | kernel の時間が出る | encoder の kernel の時間が 0 より大きい値で読める（specified、RP-7 の陽性対照） | 陽性 | `encoder_kernel_time_is_read_by_the_same_function` |
| T-RP-7-007 | fixture：int8 の model の run と fp32 の run の転送の回数 | 検知の道具が差を見ること | int8 が多い | 比べる関数が「int8 の回数 > fp32 の回数」を返す（specified、RP-7 の陰性欄） | 陰性 | `int8_run_shows_more_transfers_than_fp32_run` |
| T-RP-7-008 | `docs/results/gpu_stages.md` の実物（R1 の計測で `nsys profile --stats=true` の統計を取り込み済み。docs だけを読む。testdata も GPU も要らない） | 実物の読み取り（`--ignored` 無し） | 全行 | 転送の時間の p95 が `decode_ms` の p95 の 20% 以下（specified、RP-7）。Nsight Systems が Windows で動かなければ WSL の同じ build で数え、そのことを結果の file に書く（3.5） | 陽性 | `measured_transfers_stay_under_the_budget` |
| T-RP-7-009 | fixture：同じ peers の GPU の認識全体の p50 = 1000 µs と CPU の p50 = 1000 µs | 置き場の判定の等号 | CPU が GPU より遅くない | `CpuKept`（specified、設計の R7 の行「CPU の側が GPU より遅くなれば評議へ戻す」） | 境界、陽性 | `a_cpu_no_slower_than_the_gpu_keeps_the_placement` |
| T-RP-7-010 | fixture：GPU の p50 = 1000 µs、CPU の p50 = 1001 µs | 置き場の判定の外側 | CPU が GPU より遅い | `ReturnToCouncil`（specified、同上） | 境界、陰性 | `a_cpu_one_microsecond_slower_returns_the_placement_to_the_council` |
| T-RP-7-011 | fixture：provider と peers が同じ 2 つの run file（認識と encoder 単独）、および同じ段と variant を持つ 2 つの run file | run file の統合 | 統合する、重なれば拒む | 前者は 1 本の run に標本が足され、`peer_decodes` は和になる。後者は `Err`（derived、CUDA で 2 段が 1 process に並ばないための分割） | 陽性、陰性 | `runs_of_one_provider_and_peer_count_are_joined`、`a_stage_in_two_files_of_one_kind_is_refused` |
| T-RP-7-012 | fixture：frame 200、token 50、joiner 1 回 300 µs、decoder 1 回 100 µs。残りは範囲の下限、上限、上限を超える値、下限に満たない値 | 探索の見積りの下と上、残りの置き場 | 下 65000、上 80000 | 下 = 200 × 300 + 50 × 100 µs、上 = 200 × 300 + 200 × 100 µs。残りは両端を含めて内側、上限を超えれば `Above`、下限に満たなければ `Below`（derived。`Above` のときは差の原因を未特定と書く） | 境界、陽性、陰性 | `the_bracket_runs_from_a_decoder_call_per_token_to_one_per_frame`、`a_remainder_is_placed_against_the_bracket` |

T-RP-7-009 から 012 の test は `native/hayamimi-eval/src/r7_report.rs` の単体 test で、`cargo test --release -p hayamimi-eval --lib r7_report` で走る。

### 4.18 RP-8（CPU に残した段の根拠）

test：`native/hayamimi-eval/tests/rp08.rs`。
`check_stage_evidence(table, results)` は、`rust_port.ja.md` の 3.5 の表（`### 3.5` から `### 3.6` の間の表。見出しの行を除いて 19 行）と `docs/results/gpu_stages.md` を突き合わせる。
CPU の行は 3.5 の根拠 1〜4 のどれかで受ける。
根拠 1 は `gpu_stages.md` の同じ variant の GPU の行と CPU の行の対で CPU の p50 < GPU の p50（variant を混ぜた最小値では比べない。各 cell は 1 run で書く）、根拠 2 は GPU の `transfer-floor` の行と CPU の行の対で CPU の p50 < GPU の p50、根拠 3 は CPU の p50 < `L` の見積りの p50 の 1%（分母の `L` は見積りの値）、根拠 4（「いまの置き場」が「CPU（GPU 経路なし）」で始まる行）は、CPU の行が未測定の間は根拠の列に持ち主の slice（R2、R4、R5）の名前が在ること、測った後は CPU の p50 < GPU の往復の下限、である。
GPU の往復の下限は、test が `gpu_stages.md` の「認識の文字列 → 訳の入力」の GPU の無負荷の行の p50 の最小として読み、定数に置かない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-8-001 | 設計の 3.5 の表の実物 | 表の読み取り | 19 行 | 表の行が 19（derived、`awk` で 3.5 の節の `^\| ` の行を数え、見出しの 1 行を引いた値）。読み取りが働く証拠で、以降の判定の足場 | 陽性 | `design_table_parser_reads_19_rows` |
| T-RP-8-002 | fixture：根拠 1 の CPU の行が、同じ variant の GPU の p50 と p95、CPU の p50 と p95 を持ち、CPU の p50 < GPU の p50 | CPU を採る根拠が実測 | `Ok` | `Ok`（specified、RP-8） | 陽性 | `cpu_row_with_faster_cpu_measurement_passes` |
| T-RP-8-003 | fixture：根拠 1 の行で、同じ variant の CPU の p50 = GPU の p50 | 等号 | `Err` | `Err`。「CPU の p50 が GPU より小さい」でなければならない（specified、RP-8） | 境界、陰性 | `cpu_row_with_equal_p50_is_rejected` |
| T-RP-8-004 | fixture：根拠 1 の行で、同じ variant の CPU の p50 > GPU の p50 で一次資料の URL も無い | 根拠が無い | `Err` | `Err` でその段を名指し（specified、RP-8） | 陰性 | `cpu_row_slower_than_gpu_without_url_is_rejected` |
| T-RP-8-005 | fixture：数字は無く「GPU の道具が無い」の URL が在る CPU の行 | 一次資料の根拠 | `Ok` | `Ok`（specified、RP-8） | 陽性 | `cpu_row_with_a_primary_source_url_passes` |
| T-RP-8-006 | fixture：根拠の列が空の CPU の行 | 空欄 | `Err` | `Err`（specified、RP-8 の陰性欄） | 陰性 | `cpu_row_with_empty_evidence_is_rejected` |
| T-RP-8-007 | fixture：GPU の行が p50 と p95 を持たない | GPU の行も同じ形 | `Err` | `Err`（specified、RP-8 の「GPU に置いた段の行も同じ形で在る」） | 陰性 | `gpu_row_without_percentiles_is_rejected` |
| T-RP-8-008 | `docs/results/gpu_stages.md` と 3.5 の表の実物（docs だけを読む。testdata も GPU も要らない） | 実物の読み取り（`--ignored` 無し） | 全行 | 全行が上の形を満たす（specified、RP-8） | 陽性 | `real_stage_table_has_evidence_for_every_row` |
| T-RP-8-009 | fixture：「CPU（GPU 経路なし）」の行で、`gpu_stages.md` の CPU の行が未測定、根拠の列に `R2` と GPU の経路が無い理由が在る | 根拠 4、測る前 | `Ok` | `Ok`（specified、RP-8 の根拠 4） | 陽性 | `no_gpu_path_row_with_owner_slice_passes_before_measurement` |
| T-RP-8-010 | fixture：009 の根拠の列から `R2` を消した物 | 持ち主の欠け | `Err` | `Err` でその段を名指し（specified、RP-8 の根拠 4） | 陰性 | `no_gpu_path_row_without_owner_slice_is_rejected` |
| T-RP-8-011 | fixture：「CPU（GPU 経路なし）」の行の CPU の p50 が、GPU の往復の下限と同じ値（下限の行の p50 が 219 µs、CPU の p50 が 219 µs） | 根拠 4 の等号 | `Err` | `Err`。式：219 < 219 が偽（specified、RP-8 の陰性欄） | 境界、陰性 | `no_gpu_path_row_at_the_transfer_floor_is_rejected` |
| T-RP-8-012 | fixture：011 の CPU の p50 を 218 µs にした物 | 根拠 4 の内側 | `Ok` | `Ok`。式：218 < 219（specified、RP-8 の根拠 4） | 境界、陽性 | `no_gpu_path_row_below_the_transfer_floor_passes` |
| T-RP-8-013 | fixture：根拠 3 の行で、CPU の p50 が 13490 µs、`L` の見積りの p50 が 1349 ms | 根拠 3 の等号 | `Err` | `Err`。式：13490 µs < 1349 ms × 1% = 13490 µs が偽（specified、RP-8 の根拠 3。分母の `L` は見積りの値） | 境界、陰性 | `shrink_bound_at_one_percent_of_latency_is_rejected` |
| T-RP-8-014 | fixture：013 の CPU の p50 を 2455 µs にした物 | 根拠 3 の内側 | `Ok` | `Ok`。式：2455 < 13490（specified、RP-8 の根拠 3。3.5 の resample の行の値） | 境界、陽性 | `shrink_bound_below_one_percent_of_latency_passes` |
| T-RP-8-015 | fixture：認識の段、無負荷。GPU の fp32 の p50 が 171414 µs、CPU の fp32 が 151299 µs、CPU の int8 が 128615 µs（値は `gpu_stages.md` の「置き場の提案」の認識の行）。根拠の列は 根拠 1（variant `fp32`） | 同じ variant の対（fp32）で CPU が速い | `Ok` | `Ok`。式：fp32 の対で 151299 < 171414 が真（specified、RP-8 の根拠 1） | 陽性 | `same_variant_pair_with_faster_cpu_passes` |
| T-RP-8-016 | fixture：認識の段、無負荷。GPU の fp32 の p50 が 151299 µs、CPU の fp32 が 171414 µs、CPU の int8 が 128615 µs。根拠の列は 根拠 1（variant `fp32`） | CPU の int8 だけが速く、同じ fp32 の対では GPU が速い | `Err` | `Err` でその段を名指し。式：fp32 の対で 171414 < 151299 が偽。int8 の 128615 は対の相手が無く根拠に数えない（specified、RP-8 の陰性欄）。variant を混ぜた最小値で比べる実装は 128615 < 151299 が真で通るので、この case で落ちる | 陰性 | `cpu_int8_faster_but_same_fp32_pair_slower_is_rejected` |
| T-RP-8-017 | fixture：認識の段、無負荷。GPU に fp32（171414 µs）と `ort-encoder fp32`（66262 µs）の 2 行、CPU に fp32（151299 µs）の 1 行だけ。`ort-encoder fp32` の CPU の行は無い。根拠の列は 根拠 1（variant `fp32`） | 別の variant の行を対にしない | `Ok` | `Ok`。対は fp32 だけで 151299 < 171414 が真（specified、RP-8 の「variant を混ぜた最小値で比べない」）。GPU の最小値 66262 と CPU の 151299 を比べる実装は偽で落ちる。variant の名前を部分一致で対にする実装も `ort-encoder fp32` を fp32 の対に混ぜて落ちる | 境界、陽性 | `gpu_row_of_another_variant_is_not_mixed_into_the_pair` |
| T-RP-8-018 | fixture：認識の段、無負荷。根拠の列は 根拠 1（variant `fp32`）の 1 つだけ。fp32 の対（GPU 171414 µs、CPU 151299 µs）と `ort-encoder fp32` の対（GPU 66262 µs、CPU 121091 µs）の 2 つの対が在り、前者は CPU が速く、後者は GPU が速い。値は `gpu_stages.md` の実物 | 名指しした variant の対だけで判定する | `Ok` | `Ok`。判定に使うのは名指しした fp32 の対だけで、151299 < 171414 が真。`ort-encoder fp32` の対は GPU が速くても判定に使わない（specified、RP-8 の「同じ段のほかの variant の対は、どちらが速くても判定に使わない」） | 境界、陽性 | `named_variant_pair_alone_decides_the_evidence` |
| T-RP-8-019 | fixture：認識の段。根拠の列は 根拠 1（variant `fp32`）。GPU の行は fp16（459578 µs）だけ、CPU の行は int8（128615 µs）だけで、fp32 の行が無い | 名指しした variant の対が 0 | `Err` | `Err` でその段を名指し。対が 0 を「CPU が速い」と読まない（specified、RP-8 の「名指しした variant の対が欠けている」） | 境界、陰性 | `row_without_any_same_variant_pair_is_rejected` |
| T-RP-8-020 | fixture：018 と同じで、fp32 の対は GPU が速く（GPU 151299 µs、CPU 171414 µs）、`ort-encoder fp32` の対は CPU が速い（GPU 121091 µs、CPU 66262 µs） | 名指しの外の対が速くても救わない | `Err` | `Err` でその段を名指し。式：fp32 の対で 171414 < 151299 が偽。別の variant の対で CPU が速いことは判定に使わない（specified、RP-8） | 陰性 | `faster_cpu_pair_of_an_unnamed_variant_does_not_rescue_the_row` |
| T-RP-8-021 | fixture：認識の段で、根拠の列が 根拠 1 とだけ書き、variant の名指しが無い。fp32 の対は CPU が速い | 名指しが 0 | `Err` | `Err` でその段を名指し。対が成り立っていても、名指しが無ければ通さない（specified、RP-8 の「名指しが無い」） | 境界、陰性 | `evidence_one_without_a_named_variant_is_rejected` |
| T-RP-8-022 | fixture：認識の段で、根拠の列が 根拠 1（variant `fp32`）と 根拠 1（variant `ort-encoder fp32`）の 2 つを書く。どちらの対も CPU が速い | 名指しが 2 | `Err` | `Err` でその段を名指し。名指しは 1 つだけ（specified、RP-8 の「2 つ以上在る」） | 境界、陰性 | `evidence_one_with_two_named_variants_is_rejected` |
| T-RP-8-023 | fixture：認識の段で、根拠の列が 根拠 1（variant `fp32`）。GPU の fp32 の行が無く（CPU の fp32 は 151299 µs）、`ort-encoder fp32` の対だけが在り CPU が速い | 名指しした variant の対の片側が欠ける | `Err` | `Err` でその段を名指し。別の variant の対で埋めない（specified、RP-8 の「名指しした variant の対が欠けている」。019 は 2 側とも無い場合） | 境界、陰性 | `named_variant_pair_missing_one_side_is_rejected` |

### 4.19 RP-9（実行時の Python が 0 本）

test：`native/hayamimi/tests/rp09.rs`。
`check_descendants(tree)` は、`hayamimi` の process から親子をたどった子孫の名前に `python`（大文字小文字を区別しない）が無いことを確かめる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-9-001 | fixture：hayamimi(100) → ffmpeg.exe(101) だけ | 子孫に Python が無い | `Ok` | `Ok`（specified、RP-9） | 陽性 | `tree_without_python_passes` |
| T-RP-9-002 | fixture：hayamimi(100) → ffmpeg.exe(101) → python.exe(102) | 孫に Python | `Err` | `Err` で python.exe を名指し（specified、RP-9 の陰性欄） | 陰性 | `python_grandchild_is_rejected` |
| T-RP-9-003 | fixture：hayamimi(100) の子孫に `Pythonw.EXE` | 大文字小文字 | `Err` | `Err`（derived、名前に python を含む） | 境界、陰性 | `python_name_is_matched_ignoring_case` |
| T-RP-9-004 | fixture：親が 1 の別の process `python.exe`(200)、hayamimi の子孫ではない | 子孫でない Python | `Ok` | `Ok`。子孫だけを見る（specified、RP-9） | 境界 | `python_outside_the_tree_is_not_counted` |
| T-RP-9-005 | fixture：`dumpbin /dependents` の出力に `python311.dll` | DLL | `Err` | `Err`（specified、RP-9） | 陰性 | `python_dll_dependency_is_rejected` |
| T-RP-9-006 | fixture：`dumpbin /dependents` の出力が `KERNEL32.dll` と `sherpa-onnx-c-api.dll` だけ | DLL | `Ok` | `Ok`（specified、RP-9） | 陽性 | `dependency_list_without_python_passes` |
| T-RP-9-007 | fixture：R0 の python の run の process 一覧（`python.exe` が 1 つ） | 検知の道具の陽性対照 | 1 以上 | 同じ関数が `python.exe` を 1 以上数える（specified、RP-9 の陽性対照） | 陽性 | `python_run_sample_counts_python_exe` |
| T-RP-9-008 | fixture：`a.py` と `b.rs` と `c.py` の file 一覧 | `*.py` の数え方 | 2 | 数 = 2（derived、手で数えた値） | 陽性 | `py_file_counter_counts_two_in_a_three_file_list` |
| T-RP-9-009 | fixture：空の一覧 | 0 本 | 0 | 数 = 0 で、R11 の後の条件を満たす（specified、RP-9 の「`*.py` が 0」） | 境界 | `py_file_counter_reads_zero_for_an_empty_list` |
| T-RP-9-010 | 実走：Windows で `<hy> --input mix --serve` と `hayamimi-relay live` を 10 分、1 秒ごとに子孫の名前を集める | 実走 | 実走 | 集めた全部の名前に `python` を含む物が 0（specified、RP-9） | 陽性 | `ten_minute_run_has_no_python_descendant` |
| T-RP-9-011 | 実走：Python を子に立てる偽の build（口は実装の席が置く） | 実走の陰性 | 実走 | 子孫に python が出て `check_descendants` が `Err`（specified、RP-9 の陰性欄） | 陰性 | `build_spawning_python_is_rejected` |

### 4.20 RP-10（Windows で動く）

test：`native/hayamimi/tests/rp10.rs`。
`check_event_stream(events)` は、`/events` の受信を並べた列を確かめる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-10-001 | fixture：`ready`、`partial`、`final`（`lang` = `en`） | 期待どおりの列 | `Ok` | `Ok`（specified、RP-10） | 陽性 | `ready_then_partial_then_english_final_passes` |
| T-RP-10-002 | fixture：`final` が `ready` より前 | 順序 | `Err` | `Err`（specified、RP-10 の「`ready` の後に」） | 陰性 | `final_before_ready_is_rejected` |
| T-RP-10-003 | fixture：`final` が 0 件 | 件数の下側 | `Err` | `Err`（specified、RP-10 の「1 件以上」） | 境界、陰性 | `no_final_is_rejected` |
| T-RP-10-004 | fixture：`partial` が 0 件 | 件数の下側 | `Err` | `Err`（specified、RP-10） | 境界、陰性 | `no_partial_is_rejected` |
| T-RP-10-005 | fixture：`final` の `lang` が `ja` | 言語 | `Err` | `Err`（specified、RP-10 の `lang` が `en`） | 陰性 | `non_english_final_is_rejected` |
| T-RP-10-006 | fixture：出力の装置が無い run。終了 code 0、`capture.fallback.single` が 1 件（`reason` が `device_missing`）、`/events` の `fallback` が 1 件、マイクの `final` が 1 件以上、`error` の event が 0 件 | 出力の装置を止めた run（落ちた形） | `Ok` | `Ok`。止まらず、落ちた event が段ごとに 1 件出る（specified、RP-10 の陰性欄と 3.15 の「取り込みの装置が無い」）。検査は `check_capture_fallback(run)` | 陰性 | `missing_output_device_falls_back_to_the_microphone` |
| T-RP-10-007 | 実走：Windows 11、loopback へ英語の wav を鳴らし `/events` を読む | 実走 | 実走 | 001 の形（specified、RP-10）。マイクは入力の装置が在るときだけ同じ形 | 陽性 | `windows_loopback_reaches_events` |
| T-RP-10-008 | 実走：WSL で `--wav` の同じ test | 陽性対照 | 実走 | 001 の形（specified、RP-10 の陽性対照） | 陽性 | `wsl_wav_run_passes_the_same_check` |
| T-RP-10-009 | fixture：出力の装置もマイクも在る run。`final` が 1 件以上、`capture.fallback.single` と `capture.fallback.none` が 0 件、`/events` の `fallback` が 0 件 | 在るのに落ちない（006 の対） | `Ok` | `Ok`。装置が在るのに落ちた event が 0 件（specified、RP-10 の対照と 3.15） | 陽性 | `present_devices_emit_no_capture_fallback` |
| T-RP-10-010 | fixture：006 と同じ run で `capture.fallback.single` が 0 件 | 落ちたのに言わない | `Err` | `Err` で `capture.fallback.single` を名指し（specified、3.15 の「段ごとに出す」） | 陰性 | `silent_capture_fallback_is_rejected` |
| T-RP-10-011 | fixture：`error` の event があり、`final` が無い列 | 止まる build | `Err` | `Err` で `error` の event を名指し。止まって `error` を出す形は通さない（specified、RP-10 の「止まらず」） | 陰性 | `error_event_without_final_is_reported` |

### 4.21 RP-11（WSL で動く）

test：`native/hayamimi/tests/rp11.rs`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-11-001 | fixture：`--wav` と `/ingest` の 2 つの run それぞれが `final` 1 件以上、`gpu.session.created` の provider が 3.5 の表の置き場と全件で同じ | 両方の入力 | `Ok` | `Ok`（specified、RP-11） | 陽性 | `both_inputs_with_cuda_sessions_pass` |
| T-RP-11-002 | fixture：`/ingest` の run に `final` が 0 件 | 片方の欠け | `Err` で `/ingest` を名指し | `Err`（specified、RP-11） | 陰性 | `ingest_run_without_final_is_rejected` |
| T-RP-11-003 | fixture：`--wav` の run で、3.5 の表で GPU とした段（言語判定）の provider が `cpu` | provider | `Err` | `Err`（specified、RP-11） | 陰性 | `wav_run_on_cpu_is_rejected` |
| T-RP-11-004 | fixture：8 kHz と宣言して 16 kHz の音声を送った run の CER が、16 kHz と宣言した同じ音声の run の CER より大きい | 宣言と違う sample rate | `Err` | 比べる関数が「宣言違いの CER > 正しい宣言の CER」を `Err` で返す（specified、RP-11 の陰性欄）。許される CER の差の数は設計に無く、R1b が置く | 陰性 | `mismatched_sample_rate_declaration_raises_cer` |
| T-RP-11-005 | 実走：WSL で `--wav` と `/ingest` に同じ wav | 実走 | 実走 | 001 の形（specified、RP-11） | 陽性 | `wsl_wav_and_ingest_reach_events_on_cuda` |

### 4.22 RP-12（wire 形式の互換）

test：`native/hayamimi/tests/rp12.rs`。
`compare_event(python, rust, allowed)` は、key の集合と値の型を比べる。Rust が足してよい項目は、`final` と `refine` の `id`、`source`、`t0_s`、`t1_s`、`end_wall_ms`（会話字幕の設計 3.3）と、`translation` の `src_sha256` である。
値の型は JSON の型（null、boolean、number、string、array、object）で、整数と小数は同じ number とする。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-12-001 | `<base>/wire/` の python の event を python の event と比べる | 陽性対照 | 差 0 | 録れた型の全部で差 0（specified、RP-12 の陽性対照） | 陽性 | `python_recording_compared_with_itself_has_no_difference` |
| T-RP-12-002 | rust の `final` が python の `final` に `id` と `t0_s` を足した形 | 足してよい項目 | `Ok` | `Ok`（specified、RP-12 の「3.3 の 5 項目」） | 陽性 | `added_allowed_keys_on_final_pass` |
| T-RP-12-003 | rust の `translation` が `src_sha256` を足した形 | 足してよい項目 | `Ok` | `Ok`（specified、RP-12） | 陽性 | `added_src_sha256_on_translation_passes` |
| T-RP-12-004 | rust の `partial` が `id` を足した形 | 足してよい型の外 | `Err` | `Err` で `id` と型 `partial` を名指し（derived、足してよいのは `final` と `refine` の項目だけ） | 境界、陰性 | `allowed_key_on_the_wrong_event_type_is_rejected` |
| T-RP-12-005 | rust の `final` から `lang` を落とした形 | key の欠け | `Err` | `Err` で `lang` を名指し（specified、RP-12 の陰性欄） | 陰性 | `missing_key_is_named` |
| T-RP-12-006 | rust の `final` の `audio_s` が string | 値の型の違い | `Err` | `Err` で `audio_s` を名指し（specified、RP-12） | 陰性 | `changed_value_type_is_named` |
| T-RP-12-007 | python の値 `1`、rust の値 `1.0` | 整数と小数 | `Ok` | `Ok`。同じ number（derived、JSON の型） | 境界 | `integer_and_float_are_the_same_json_type` |
| T-RP-12-008 | python の値が null、rust の値が string | null の型 | `Err` | `Err`（derived、null は独立した型） | 境界、陰性 | `null_and_string_are_different_types` |
| T-RP-12-009 | python が録った型の全部の rust の出力 | 型の網羅 | 全部に rust の例 | rust の出力に無い型があれば `Err` でその型を名指し（specified、RP-12 の「録れた event の型ごとに」）。fixture は `refine` を欠く | 陰性 | `type_without_a_rust_sample_is_named` |
| T-RP-12-010 | 8 本の route と `/ingest` の handshake の応答 | route の比べ | 同じ status と key | 同じ（specified、RP-12）。fixture の 1 本だけ status が違う物は `Err` でその route を名指し | 陽性、陰性 | `routes_and_handshake_match_status_and_keys` |
| T-RP-12-011 | 実走：mobile の `remote_event_test.dart` の fixture を rust の出力で置き換えて `dart test` | 実走 | 実走 | 全 test が通る（specified、RP-12。外部の道具の判定） | 陽性 | `mobile_dart_tests_pass_with_rust_fixtures` |
| T-RP-12-012 | rust の出力に `{"type": "fallback", "stage": …, "reason": …, "to": …}` の event が在り、python の録りに `fallback` の型は無い | 3.15 の型は差から除く | `Ok` | `Ok`。型の網羅（009）と key の比べの対象に入れない（specified、RP-12 の「3.15 の `fallback` の event の型は差から除く」） | 境界、陽性 | `fallback_event_type_is_excluded_from_the_difference` |
| T-RP-12-013 | 012 の出力で、`final` から `lang` を落とした形 | `fallback` の除外が他の差を隠さない | `Err` | `Err` で `lang` を名指し（specified、RP-12 の陰性欄。005 と同じ差が、`fallback` の event が在っても落ちる） | 陰性 | `fallback_exclusion_does_not_hide_a_missing_key` |

### 4.23 RP-13（訳の品質）

test：`native/hayamimi-eval/tests/rp13.rs`。
`check_chrf(record, measured)` は、corpus chrF が記録を超える（等号は含まない）ことを言語ごとに確かめる。記録は en 47.57、zh 20.19、ko 25.20、es 42.09。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-13-001 | 測った値が全言語で記録 + 0.01 | 超える側の最小の桁 | `Ok` | `Ok`（specified、RP-13 の「超える」） | 境界、陽性 | `values_one_hundredth_above_record_pass` |
| T-RP-13-002 | en が 47.57（記録と同じ） | 等号 | `Err` | `Err` で en を名指し。超えていない（specified、RP-13） | 境界、陰性 | `value_equal_to_record_is_rejected` |
| T-RP-13-003 | zh が 20.18 | 記録の下側 | `Err` | `Err` で zh を名指し（specified、RP-13） | 陰性 | `value_below_record_is_rejected` |
| T-RP-13-004 | 50 文のうち V が両方の訳を止めた 3 文の hyp が空 | 空の hyp の数え方 | n = 50 | corpus の対の数が 50 のまま（空の hyp を落とさない。specified、RP-13） | 境界 | `empty_hypotheses_are_counted_not_dropped` |
| T-RP-13-005 | 50 文とも hyp が空 | 全部空 | 0 | corpus chrF = 0.0（NOT VERIFIED。`<py> -m sacrebleu <ref> -i <empty> -m chrf -b` の 1 行で確かめる） | 境界 | `all_empty_hypotheses_score_zero` |
| T-RP-13-006 | hyp の file が 49 行（ref は 50 行） | 行数の食い違い | `Err` | `Err` で行数 49 と 50 を名指し（implicit） | 陰性 | `hypothesis_line_count_mismatch_is_rejected` |
| T-RP-13-007 | 参照訳を hyp に渡す | 陽性対照 | 100 | 4 言語とも 100.0（specified、RP-13 の陽性対照） | 陽性 | `reference_as_hypothesis_scores_100` |
| T-RP-13-008 | 原文を hyp に渡す | 陰性対照 | 記録未満 | 4 言語とも記録を割る（specified、RP-13 の陰性欄）。zh は漢字を共有するので、割らない可能性は R0 の実測で確かめる（RB-7 の 017 と同じ式） | 陰性 | `source_as_hypothesis_stays_below_record` |
| T-RP-13-009 | 実走：`hayamimi-relay eval --pairs pairs/ --out hyps/` の後に `<ev> chrf` | 実走 | 実走 | 4 言語とも記録を超える（specified、RP-13。llama-server が要る。`eval` は R8f で足す） | 陽性 | `translation_chrf_beats_current_models` |
| T-RP-13-010 | 測っていない言語が 1 つある測定値 | 言語の欠け | `Err` | `Err` で欠けた言語を名指し（implicit） | 陰性 | `a_language_that_was_not_measured_is_named` |
| T-RP-13-011 | 劣化させた hyp、原文、参照訳の 3 通りを 4 言語で採点 | 採点器が sacrebleu 2.6.0 と同じ値を出すこと | 差 ≤ 0.01（4 桁の記録の丸め以内） | 文ごとの 200 値と corpus の 12 値が `<base>/chrf_scores/` と一致（derived。RP-14 の材料と同じ） | 陽性 | `the_scorer_gives_the_recorded_sacrebleu_scores` |
| T-RP-13-012 | `hayamimi-relay eval` を偽の llama-server に流す（`native/hayamimi-relay/tests/r8f_eval.rs`）。(1) `en.ja.txt` の 3 文（E が V を通る文、E が V に落ちて T が通る文、T と E の両方が V に落ちる文）。(2) `es.ja.txt` の 2 文（訳の数字が原文と合う文、合わない文）。server は、user message が `Translate the following` で始まる依頼に T の答え、それ以外に E の答え、`chat_template_kwargs` を持つ依頼に es の答えを返す | 1 文の確定の訳。ja と en の組（en）は E、T、空のどれか。組の外（es）は T と V だけで、T、空のどちらか | en は 3 文、es は 2 文 | en：E が V を通れば E、通らず T が通れば T、両方外れれば空行。要求は 6 本（文ごとに T と E）。es：T が V を通れば T、外れれば空行（`held`）。要求は 2 本（文ごとに T だけで、E を掛けない）。どちらも行数は原文と同じで、`Written` の `held` は en が 1、es が 1（derived、RP-13 の「V が止めた行は空の hyp」と、設計 3.6 の「ja と en の外の言語の訳は T と V だけ」） | 陽性、境界 | `each_sentence_settles_on_the_edit_then_the_draft_then_an_empty_line`、`a_language_outside_the_pair_settles_on_t_alone_or_an_empty_line` |
| T-RP-13-013 | 同じ偽の server に 1 文ずつ流し、要求の body を読む。(1) `en.ja.txt` の 1 文。(2) `zh.ja.txt` の 1 文（`猫が寝ています。`） | 依頼の形。組の中（en）と組の外（zh） | en は user message 1 つ、zh は user message 1 つと言語の code | en：「Translate the following Japanese text into English.」で始まる message が 1 つ、2 本目は「Rewritten English:」を含む E の依頼、`stop` が `["</s>"]`、`temperature` が 0、`cache_prompt` が真。zh：要求は 1 本で、`messages` が `[{"role":"user","content":"猫が寝ています。"}]`（指示の文を付けない原文だけ）、`chat_template_kwargs` が `{"source_lang_code":"ja","target_lang_code":"zh"}`、`stop`、`temperature`、`cache_prompt` は en と同じ（specified、設計 3.6 の「訳の依頼の形」と、組の外の言語の依頼） | 陽性（en と zh で形が違うこと。zh の message に指示の文が入ると落ちる） | `a_request_names_the_language_in_one_user_message`、`a_request_outside_the_pair_carries_the_language_codes` |
| T-RP-13-014 | pairs に表に無い言語（`xx`）が混ざる | 訳せない言語 | 1 言語 | その言語を訳さず `translate.fallback.skipped`（warn、`HAYAMIMI_TRANSLATE_FALLBACK`、`reason` が `unsupported_target`）を 1 件出し、ほかの言語は続く（specified、設計 3.15 の止めない形） | 陰性 | `an_unlisted_language_is_left_out_with_an_event_and_the_others_go_on` |
| T-RP-13-015 | llama-server に届かない、または要求が 4xx | 測定を止める場合 | 1 行目、2 行目 | `Err` で言語と行番号を名指しし、hyp の file を書かない（derived。空の答えの採点が測定に見えるのを防ぐ） | 陰性 | `an_unreachable_translator_stops_the_run_without_writing`、`a_refused_request_names_the_language_and_the_line` |
| T-RP-13-016 | `native/hayamimi-relay/tests/r8f_eval.rs`。014 と同じ偽の server に、`en.ja.txt`、`zh.ja.txt`、`ko.ja.txt`、`es.ja.txt` の 4 file（各 1 行。原文は数字とラテン文字の名前を持たない。server の答えは、en と es がラテン文字、zh が漢字、ko がハングルの文で、V を通る形）だけを置いた pairs を流す。`xx.ja.txt` は置かない | 表に在る言語だけの pairs（014 は表に無い `xx` が混ざる pairs）。RP-13 の 4 言語で、live の組に在る en と、組の外の zh、ko、es を両側に含める | `translate.fallback.skipped` が 0 件 | `report.skipped` が空、`translate.fallback.skipped` の件数が 0、`<code>.hyp.txt` が 4 本、server が受けた要求が 5 本（en は T と E の 2 本、組の外の zh、ko、es は T だけの各 1 本）（specified、設計 3.15 の表の「表に無い言語が在る」ときだけ出す。要求の数は derived、012 の式） | 陰性（014 の対）、陽性（同じ sink が `translate.eval.written` を 4 件持つ。0 件が event の名前の違いではないことを示す。014 の同じ絞り込みは `xx` を足した pairs で 1 件を数える） | `pairs_of_listed_languages_only_emit_no_skipped_event` |
| T-RP-13-017 | `native/hayamimi-relay/tests/r8f_live.rs`。偽の recognizer が `/events` に ja の `final` を 1 件（`id` 付き。原文は `猫が寝ています。`）流す。偽の llama-server は、`chat_template_kwargs` の `target_lang_code` が `zh` の依頼に `猫在睡觉。`、それ以外の依頼に英語の文を返す。引数は `live --events <偽の recognizer> --llm <偽の server> --llm-other <偽の server> --serve 0 --translate zh` で、`parse_args` が返した設定を、`cli::run` が使うのと同じ「中継を起こして handle を返す関数」（`start_live`）に渡して起こし、bind した port の `/events` を読み、handle で止める（走り続ける `cli::run` を test から直に起こさない） | `--translate` の言語が、表に在り、live の組（ja と en）の外に在る（zh）。流す行は 1 件（0 件でも 2 件でもない境界） | `lang` が `zh` の確定の訳（`settled` が真）が 1 件、`lang` が `en` の `translation` が 0 件、server が受けた要求が 1 本 | `/events` の `translation` のうち、`settled` が真で `lang` が `zh` の物がちょうど 1 件で、`lang` が `en` の `translation`（先の訳を含む）は 0 件（`--translate` は訳の先の組を置き換える。設計 3.6）。zh の event の `id` が流した `final` の `id`、`src_sha256` が原文の sha256、`status` が `ok`、`text` が server の答え。server が受けた要求は 1 本で、user message が原文だけ、`target_lang_code` が `zh`（T だけで E を掛けない。設計 3.6）（specified、R8f の受入。会話字幕の設計 3.3 の `translation` の項目） | 陽性（要求が `target_lang_code` が `zh` の 1 本であること。event が組の外の言語の経路を通って作られたことの対照）、陰性（`--translate` を付けない同じ入力では、`lang` が `zh` の `translation` が 0 件で、確定の訳が 1 件あり `lang` が `en`。`zh` の 0 件が collector の空ではないことの対照も兼ねる。設計 3.6 の既定の規則：mic の ja は en へ）、陽性（`--translate en,zh` では `en` と `zh` の確定の訳が 1 件ずつ。en の 0 件が en を落とす癖ではなく置き換えの結果であることの対照） | `a_translate_option_outside_the_live_pair_publishes_a_translation_in_that_language`、`without_the_translate_option_no_translation_in_that_language_is_published`、`translate_en_and_zh_publishes_both` |
| T-RP-13-018 | `native/hayamimi-relay/tests/r8f_live.rs`。017 と同じ偽の recognizer と偽の server、同じ起こし方で、引数は `live --events <偽の recognizer> --llm <偽の server> --llm-other <偽の server> --serve 0 --translate xx,zh`。ja の `final` を 1 件流した後に、`/events` へもう 1 本 client を繋ぐ（遅れて繋いだ client） | 表に在る言語（zh）と表に無い言語（`xx`）が混ざる。表に無い言語は 1 つ（0 個は 017 の入力、2 個以上は設計に値が無いので数えない）。client は流す前から繋いだ物と、流した後に繋いだ物の 2 本 | `xx` について log の `translate.fallback.skipped` が 1 件、`/events` の `fallback` が 1 件 | log：`translate.fallback.skipped`（warn、`reason` が `unsupported_target`、`error_code` が `HAYAMIMI_TRANSLATE_FALLBACK`）が `target` が `xx` で 1 件。中継の起動の後の `skipped()` が `["xx"]`。`/events`：`{"type":"fallback","stage":"translate","reason":"unsupported_target","to":"skipped"}` が最初の client に 1 件、遅れて繋いだ client にも 1 件（取っておいた event として最初にまとめて届く）。zh の確定の訳は 1 件出る。server が受けた要求は 1 本で、`target_lang_code` が `zh`（`xx` の依頼は 0 本。specified、設計 3.15 の表の `live` の行と、R8f の受入） | 陽性（zh の確定の訳が 1 件出ること。ほかの言語が続く対照）、陰性（`--translate zh` だけの起動では `fallback` が 0 件で log の `translate.fallback.skipped` も 0 件。`fallback` の 1 件が数え違いではないことの対照。遅れて繋いだ client が `fallback` を受けないと落ちる） | `an_unlisted_translate_language_is_skipped_with_a_kept_fallback_event_and_zh_goes_on`、`a_run_that_names_only_zh_has_no_fallback_event` |
| T-RP-13-019 | `native/hayamimi-relay/tests/chat_probe_gemma.rs`。会話字幕の probe の 40 文（`testdata/conv20.tsv`）を、中継の組の外の言語向けの依頼（原文だけの user message と言語の code）で 2 本目の llama-server（port 18081、または `testdata/rp13_other_server.txt` の URL）に流した訳と、同じ body を `curl` で打った訳を比べる。body だけの 2 本は server を要らない | 中継の依頼の body が `curl` の body と同じであること。server に打つ 3 本は実走 | 実走（3 本）。body の 2 本は server 無し | server 無し：中継の body と `curl` の body が JSON として等しい（原文が ja の文と en の文の 2 通り）。実走：中継の訳が 40 文の全てで `curl` の訳と同じ。`curl` の訳が probe の log の訳と同じ行の数を記録し、合否は中継と `curl` の一致で決める（specified、設計 3.6 の「訳の依頼の形」の受入と同じ形） | 陽性（`curl` の訳が probe の log の `j01` の答え「Hello.」と同じ）、陰性（言語の code を付けず、組の中の言語の prompt を user message にした同じ 40 文では、`curl` と違う行が 1 行以上。code の向きを入れ替えた body は `curl` の body と等しくない） | `the_relay_sends_the_body_curl_sends`、`a_body_with_the_codes_swapped_is_not_the_body_curl_sends`、`coded_t_matches_curl_on_every_probe_sentence`、`curl_matches_the_probe_log`、`the_form_without_codes_differs_from_curl_on_some_probe_sentence` |
| T-RP-13-020 | `native/hayamimi-relay/tests/r8f_live.rs`。017 と同じ偽の recognizer と起こし方で、`--llm-other` を既定の address（`http://127.0.0.1:18081`）にし、そこに `hayamimi-llama-other.service`（TranslateGemma-4B-it Q8_0）を立てる。引数は `live --events <偽の recognizer> --llm <偽の server> --serve 0 --translate zh --llm-other <既定>`。ja の `final` を 1 件流す | 2 本目の server の実走。組の外の言語は zh の 1 つ、流す行は 1 件 | `lang` が `zh` の確定の訳が 1 件 | `/events` の `translation` のうち `settled` が真で `lang` が `zh` の物がちょうど 1 件、`status` が `ok`、`text` が漢字を含みかなを含まない。組の側の偽の server が受けた要求は 0 本（`#[ignore]`、server を立てた時に `--ignored` で打つ。測定：`猫が寝ています。` → `猫在睡觉。`）（specified） | 陽性（実 server の訳が漢字だけで書かれていること）、陰性（組の側の server に要求が 0 本であること。zh の依頼が 2 本目だけへ行った対照） | `the_real_second_server_translates_a_japanese_line_into_chinese` |
| T-RP-13-021 | `native/hayamimi-relay/tests/r8f_live.rs`。020 と同じ入力で、`--llm-other http://127.0.0.1:1`（誰も聞いていない port） | 2 本目が立たない場合 | zh の確定の行が 1 件 | 中継は止まらず、zh の `translation` が 1 件で `status` が `held`（`ok` ではない）、log の `translate.request.failed` が 1 件（specified、設計 3.6 の落ちて動く規則） | 陽性（中継が止まらず訳の event が 1 件出ること）、陰性（`ok` の訳が 0 件であること。020 は `ok` が 1 件で、2 本は同じ入力で server の有無だけが違う） | `an_absent_second_server_holds_the_line_and_says_the_translator_was_unreachable` |
| T-RP-13-022 | `fleurs::tsv_url("es_419", Split::Test)`（`native/hayamimi-eval/src/fleurs.rs` の unit test） | test の表の場所 | 1 通り | `https://huggingface.co/datasets/google/fleurs/resolve/main/data/es_419/test.tsv`（specified、H36。HEAD で 200 を確かめた。ja_jp の test.tsv は 361174 byte。本文は取っていない） | 陽性 | `the_test_split_url_names_the_test_table` |
| T-RP-13-023 | `Split::default()` と `tsv_url("ja_jp", Split::default())` | 既定の split | dev | `Split::Dev` で、URL は `.../data/ja_jp/dev.tsv`（specified、既存の run の組を変えない）。手で打つ対：`<ev> fleurs-pairs --targets en,zh,ko,es --n 50 --seed 0 --degrade --out <dir>` の 16 file が `testdata/baseline/pairs` と `cmp` で全部一致する | 陽性、回帰 | `the_default_split_is_the_dev_table_of_the_recorded_runs` |
| T-RP-13-024 | `fleurs-pairs --targets en --out p` に `--split` を付けない、`--split test`、`--split dev` を `Cli::try_parse_from` に渡す（`native/hayamimi-eval/src/cli.rs` の unit test） | 省略、test、dev | 3 通り | 省略は `Dev`、`test` は `Test`、`dev` は `Dev`（specified、H36） | 陽性、境界 | `fleurs_pairs_without_a_split_reads_dev`、`fleurs_pairs_split_test_reads_test_and_dev_reads_dev` |
| T-RP-13-025 | `--split train2` | 表に無い値 | `Err` | `Err` の種類が `InvalidValue` で exit code が 2（使い方の誤り。specified、H36） | 陰性 | `an_unknown_split_is_a_usage_error` |
| T-RP-13-026 | `fleurs-pairs` の `--help` | 使い方の表示 | 1 通り | `--split` と `[default: dev]` を含む（specified、H36） | 陽性 | `fleurs_pairs_help_lists_the_split_option` |
| T-RP-13-027 | `hayamimi-relay vprobe --pairs <dir> --lang en`（`native/hayamimi-relay/tests/vprobe.rs`）。en の 5 文：前置き付きの参照訳（id 50）、数字を含む行（20）、否定の語を含む行（30）、推量の語を含む行（40）、どれも含まない行（10） | 参照訳をそのまま V に通す | 1 通り | `reference` の行は rows=5、止まった id は `50` だけ（specified、H36） | 陽性 | `the_references_v_withholds_as_they_stand_are_named` |
| T-RP-13-028 | 同じ 5 文 | 壊し方ごとの対象行と止まり方 | 3 通り | `break:number` は id 20、`break:negation` は 30、`break:uncertainty` は 40 だけが対象（rows=1）で、3 本とも V が止める（specified、H36）。陰性対照：壊し方の 1 つを壊さない形に Edit すると、その壊し方の止まった行が 0 になり落ちる（3 通りとも実施） | 陽性、陰性 | `each_break_is_counted_and_withheld_by_its_own_rows` |
| T-RP-13-029 | 偽の server が 20 に「4 時」、30 に否定の落ちた訳を返す | T だけの訳を V なしと V ありで書く | 5 文 | V を通さない file は 5 行そのまま、V を通す file は id 20 と 30 の行が空（specified、H36）。コマンドは `noV/<lang>.hyp.txt` と `V/<lang>.hyp.txt` を書く | 陽性 | `t_alone_is_written_whole_and_gated_with_withheld_lines_empty`、`the_command_writes_the_two_hypothesis_directories` |
| T-RP-13-030 | 届かない server、長さの違う 3 つの file | 失敗 | 2 通り | T は言語を名指しして止まる。ids、ja、ref の行数が違えば 3 つの行数を名指しして断る（implicit） | 陰性 | `an_unreachable_translator_stops_t_alone_and_names_the_language`、`files_of_different_lengths_are_refused` |
| T-RP-13-031 | 数を 1 字ずらす、否定と推量の標識を落とす、標識の語の一部（`knot`）、除外する句（`Not only`）、接尾辞（`n't`）、zh の部分一致、引数の欠け | 壊し方の規則と引数 | 各 1〜4 通り | 規則の出力は `vprobe.rs` の unit test の値のとおり（specified、H36） | 境界、陰性 | `the_number_rule_moves_the_first_digit_by_one`、`the_negation_rule_removes_a_word_marker_only_as_a_whole_word`、`a_phrase_the_table_excludes_is_not_a_negation`、`a_suffix_marker_counts_inside_a_word`、`a_substring_language_loses_its_marker_wherever_it_stands`、`the_uncertainty_rule_reads_the_uncertainty_table`、`the_options_need_pairs_and_a_language_of_the_table`、`the_options_default_to_the_relays_translators_and_no_output`、`the_file_has_one_line_per_row_and_the_report_names_the_ids` |
| T-RP-13-032 | `native/hayamimi-relay/src/pipeline.rs` の unit test。`translate_prompt(Lang::Ja, "こんにちは。")` と、`translate_prompt_to(Lang::Ja, <en>, "こんにちは。")` | T の prompt を組む 2 つの入口。空行の直後の空白の有無 | 1 通り（2 つの入口で同じ文字列） | `Translate the following Japanese text into English.\n\n こんにちは。`（空行の直後に半角空白が 1 つ。specified、設計 3.6 の「訳の依頼の形」） | 陽性、陰性（builder の空白を外す Edit で、032、033、034 の 6 本を含む 47 本が落ちることを実施した。残りの 41 本は、`tests/support/events.rs` の `read_prompt` が空白付きの形だけを T の prompt として読むため、偽の翻訳器を使う test が落ちた物） | `the_translate_prompt_follows_the_model_card`、`the_prompt_for_a_named_target_has_the_same_space` |
| T-RP-13-033 | 同じ unit test module の偽の翻訳器（`Scripted`）に、ja の確定文 `こんにちは。` を `run_staged` で流す。別の test は速報の `translate_partial` で同じ文を流す | 確定の経路の T と E、速報の経路の T | 確定は T と E の 2 本、速報は T の 1 本 | 確定：T の依頼が 032 の文字列。E の依頼は `Below is a Japanese sentence from a live conversation and a draft English translation.\n` で始まり、`\nJapanese:\nこんにちは。\n\nDraft English:\nHello.\n\nRewritten English:\n` で終わり、`\n\n ` を含まない（E は空白を足さない）。速報：依頼が 1 本で 032 の文字列（specified、設計 3.6） | 陽性、陰性（E に空白が入ると落ちる） | `the_final_path_asks_t_with_the_space_and_e_without_it`、`a_text_in_progress_is_asked_with_the_space` |
| T-RP-13-034 | `native/hayamimi-relay/tests/r8f_eval.rs`。013 と同じ偽の server に `en.ja.txt` の 1 文（`猫が寝ています。`）を流し、2 本の要求の body を読む | `eval` の経路の T と E | 要求は 2 本 | T の message は `Translate the following Japanese text into English.\n\n 猫が寝ています。`。E の message は `\nJapanese:\n猫が寝ています。\n\nDraft English:\nThe cat sleeps.\n\nRewritten English:\n` で終わり、`\n\n ` を含まない（specified、設計 3.6）。組の外の言語（zh）は 013 のとおり原文だけで、空白を足さない | 陽性、陰性（空白を外す Edit で落ちる。組の外の zh の message に空白が入ると 013 の 2 本目が落ちる） | `eval_asks_t_with_a_space_after_the_blank_line_and_e_without_one`、`a_request_names_the_language_in_one_user_message` |
| T-RP-13-035 | `native/hayamimi-relay/src/cli.rs` の unit test。`live --llm http://127.0.0.1:1 --llm-other http://127.0.0.1:2 --serve 8835` の設定から、待ち受けの event を組む。続けて `--llm`、`--llm-other` を省いた設定と、要求の port と違う port で待った設定 | 起動の event の `llm` と `llm_other`。既定の address。port が移った時の warn | 3 通り | `relay.serve.started` が `llm` に `http://127.0.0.1:1`、`llm_other` に `http://127.0.0.1:2` を持ち、`port`（8835）と `scope`（`loopback`）は変わらない。省いた時は既定（`http://127.0.0.1:8080`、`http://127.0.0.1:18081`）。port が移った時の `server.fallback.port`（warn）も同じ 2 つを持つ（derived。run が向いた translator を log から読めるようにする）。実走は `hayamimi-relay live --serve 0 --llm http://127.0.0.1:1 --llm-other http://127.0.0.1:2` の stdout の 1 行目 | 陽性、境界 | `the_start_event_names_the_two_translators_it_was_given`、`the_start_event_names_the_default_translators_when_none_is_given`、`the_port_moved_warning_names_the_translators_too` |

### 4.24 RP-14（Rust の chrF が sacrebleu と同じ値を出す）

test：`native/hayamimi-eval/tests/rp14.rs`。
fixture は R0 の `<base>/chrf_fixture.tsv`（4 言語 × 50 文の文ごとの値と 4 つの corpus の値）。
sacrebleu の `-b` の既定は小数 1 桁（NOT VERIFIED。`<py> -m sacrebleu --help` の `--width` で確かめる）で、0.01 以内を測るには R0 が桁数を増やして書く必要がある。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-14-001 | `chrf_fixture.tsv` の文ごとの 200 行 | 文ごとの値 | 差 ≤ 0.01 | 全行で Rust の値と tsv の値の差の絶対値 ≤ 0.01（specified、RP-14。oracle は sacrebleu 2.6.0） | 陽性 | `sentence_scores_match_sacrebleu` |
| T-RP-14-002 | `chrf_fixture.tsv` の corpus の 4 行 | corpus の値 | 差 ≤ 0.01 | 4 言語とも差 ≤ 0.01（specified、RP-14） | 陽性 | `corpus_scores_match_sacrebleu` |
| T-RP-14-003 | 数の組 (47.57, 47.58) と (47.57, 47.59) | 差の境界 | 0.01 | 前者は `Ok`、後者は `Err`（derived、等号側と最小の桁の外側） | 境界 | `difference_boundary_is_0_01` |
| T-RP-14-004 | tsv の最初の文 1 本 | 陽性対照 | 一致 | その 1 文の値が一致（specified、RP-14 の陽性対照） | 陽性 | `first_fixture_sentence_matches` |
| T-RP-14-005 | 参照 = hyp の文 | 完全一致 | 100 | 100.0（derived、chrF の定義で n-gram が全部一致） | 境界 | `identical_sentences_score_100` |
| T-RP-14-006 | 参照 `aaaa`、hyp `bbbb` | 共通の文字が無い | 0 | 0.0（derived、一致する n-gram が無い） | 境界 | `disjoint_characters_score_zero` |
| T-RP-14-007 | n-gram の上限を 5 にした実装を tsv の全文に掛けた結果 | 検知の道具が差を見ること | 差 > 0.01 の文が 1 件以上 | 上限 6 の実装との差が 0.01 を超える文が 1 件以上（specified、RP-14 の陰性欄。chrF の文字 n-gram の上限は 6） | 陰性 | `order_five_implementation_differs_on_some_sentence` |

### 4.25 RP-15（話者の分離）

test：`native/hayamimi-eval/tests/rp15.rs`（001〜011）と `native/hayamimi/tests/rp15_speaker.rs`（012、013）。
012 と 013 を `hayamimi` の crate に置くのは、`native/hayamimi/Cargo.toml` が `hayamimi-eval` を依存に持ち、eval の test から `<hy>` を起こせないからである。
`<base>` の Python の平均 DER は `der_eval.log` の `mean DER` の行（RB-7）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-15-001 | rust の平均 DER が python と同じ | 等号 | `Ok` | `Ok`（specified、RP-15 の「以下」） | 境界、陽性 | `equal_mean_der_passes` |
| T-RP-15-002 | python の 3 run の幅が 0.05、rust が python の平均より 0.1 pt 大きい | 幅を超えて上側 | `Err` | `Err`（specified、RP-15。差 0.1 が幅 0.05 以上） | 境界、陰性 | `mean_der_one_tenth_worse_is_rejected` |
| T-RP-15-003 | rust が 5 会議のうち 4 会議の結果 | 会議の数 | `Err` | `Err`（specified、RP-15 の「AMI 5 会議」） | 境界、陰性 | `four_meetings_are_rejected` |
| T-RP-15-004 | 参照 話者 A が 0 から 10 秒、hyp 話者 A が 0 から 5 秒と話者 B が 5 から 10 秒、collar 0 | DER の関数の手計算 | 0.5 | DER = 0.5。式：混同 5 秒 ÷ 参照の発話 10 秒（derived、手計算。collar 0 では simpleder も同じ値になる見込みだが NOT VERIFIED） | 陽性 | `der_of_half_confused_speech_is_0_5` |
| T-RP-15-005 | 参照と hyp が同じ | 完全一致 | 0 | DER = 0（derived） | 境界 | `der_of_identical_segments_is_zero` |
| T-RP-15-006 | `<base>` の `der_eval.log` と `der_eval_run<n>.log` | 陽性対照 | 3 回以上 | `--method refine_diarize` の run が 3 回以上在り、同じ test が 3 回以上の平均 DER と幅（最大 − 最小）を出せる。`diarization.md` の値は比べない（specified、RP-15 の陽性対照。run の数と method の判別と平均と幅は RB-7 の 005、020、021 と同じ関数） | 陽性 | `python_der_runs_yield_mean_and_spread_from_three_or_more_refine_diarize_runs` |
| T-RP-15-007 | fixture：清書の再分離を切った run の DER が python より悪い | 再分離を切った run | `Err` | `Err`（specified、RP-15 の陰性欄） | 陰性 | `rediarization_off_fixture_is_worse_than_python` |
| T-RP-15-008 | 実走：`<ev> der --set testdata/eval_diar --collar 0.25` | 実走 | 実走 | 平均 DER から python の 3 回以上の run の平均を引いた差が、python の幅より小さい。差と幅を結果の file に書く（specified、RP-15） | 陽性 | `rust_mean_der_not_worse_than_python` |
| T-RP-15-009 | fixture：python の DER の run が 2 つ | 幅を決められない側 | `Err` | `Err` で 2 回と言う。2 回では幅を決めない（specified、RP-15） | 境界、陰性 | `two_python_der_runs_cannot_fix_a_spread` |
| T-RP-15-010 | fixture：python の 3 run が平均 13.3、13.1、13.5（平均 13.30、幅 0.4）。rust の平均が 13.69 | 平均より 0.39 pt 悪い（幅より小さい） | `Ok` | `Ok`。結果の file に差 0.39 と幅 0.4 を書く（specified、RP-15 の「差が幅より小さい」） | 境界、陽性 | `worse_than_python_mean_by_less_than_the_spread_passes` |
| T-RP-15-011 | 同じ fixture で rust の平均が 13.70 | 平均より 0.40 pt 悪い（幅と等しい） | `Err` | `Err`（差が幅と等しいのは「幅より小さい」に入らない。specified、RP-15） | 境界、陰性 | `worse_than_python_mean_by_exactly_the_spread_is_rejected` |
| T-RP-15-012 | 実走（`native/hayamimi/tests/rp15_speaker.rs`）：`<hy> --wav tests/golden/ja/ja_014.wav --no-realtime --mode single --lang ja` を話者の model の在る directory で 1 回 | 確定文が話者を待たない | 実走 | 各 `id` で、log の `asr.final.published` の `mono_us` が、同じ `id` の `speaker.embedding.started` の `mono_us` 以下（確定文を出し終えてから埋め込みを始める）。各 `final` の `speaker` が空で、同じ `id` の `speaker` の event がその `final` の後に 1 件（specified、設計 8 の R4 の行。時刻の前後で見るので run ごとの雑音に依らない） | 陽性 | `final_does_not_wait_for_the_speaker` |
| T-RP-15-013 | fixture：`speaker.embedding.started` の `mono_us` が同じ `id` の `asr.final.published` より前の log（`final` の `speaker` は空） | 埋め込みの後に確定文を出す形 | `Err` | `Err` でその `id` と 2 つの時刻を名指し（specified、同上。`speaker` を空にしたまま待つ形も、時刻の前後で落ちる） | 陰性 | `final_waiting_for_the_speaker_is_rejected` |

### 4.26 RP-16（GPU の無い機械では段ごとに CPU へ落として字幕を出す）

test：`native/hayamimi/tests/rp16.rs`。受入 test の名前は `rp16_no_gpu_falls_back_to_cpu_and_says_so`。
`check_no_gpu_run(run)` は、終了 code、stdout の JSONL の event、`/events` の `fallback` の event、stderr の文を確かめる。
run は、その run が読み込んだ 3.5 の GPU の段の一覧（R2 の ja の run では句読点、R3 以降は言語判定も）を持つ。落ちた event はこの一覧の段ごとに 1 件を求め、読み込んでいない段には求めない。
`check_gpu_run(run)` は GPU の在る run を確かめる。

訳の行（R8b）は、llama-server を起こす command（`hayamimi llama start --model <file> --port <n> --dir <root>`）の run を同じ形で確かめる（T-RP-16-038 から 051）。
受入 test の名前は `rp16_no_gpu_translate_server_falls_back_to_cpu_and_says_so`（GPU を隠した run を ja と C.UTF-8 の 2 つの言語で打ち、どちらも 038 の形で、stderr の言語が分かれる）。
`check_translate_on_cpu(run, 理由)` は、終了 code、stdout の JSONL の event、起こした server の引数、stderr の文を確かめる。
理由が `no_device` の run は `gpu.device.unavailable` を 1 件（`reason` = `no_device`）、`session_failed` の run は 0 件求める。
`check_gpu_translate_run(run)` は GPU の在る run を確かめる。
run は、command の出力と、起こした server が log の file の最初の行に出した引数を持つ。
実走の server は test の代役（`fake_llama_server`）で、model の file の名前で動きが決まる（`exits.gguf` は終了 code 3、`silent.gguf` は待ち受けない、ほかは `/health` に `ok` を返す）。
代役は GPU を使わないので、`-ngl 0` を受けたことは引数の行で確かめる。
実物の llama-server の run は手で打ち、出力を `docs/results/r8b/` に置く（手順は `docs/guide/llama_windows.ja.md`）。
`/events` の `fallback` は、起こす command が字幕の server を持たないので求めない。
字幕の server と起こす command を繋ぐ slice が足す（NOT VERIFIED：それまで、訳の段の `/events` の `fallback` は 0 件）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-16-001 | fixture：終了 code 0、`final` 1 件以上、`gpu.device.unavailable` 1 件（`error_code` = `HAYAMIMI_GPU_DEVICE_UNAVAILABLE`、`reason` = `no_device`）、読み込んだ GPU の段は句読点だけで `gpu.fallback.cpu` 1 件と `/events` の `fallback`（`to` = `cpu`）1 件、`gpu.session.created` の実際の provider が全件 `cpu`、stderr に `nvidia-smi` | 落ちた形が揃う（落ちた形の対の片方） | `Ok` | `Ok`（specified、RP-16） | 陽性 | `no_gpu_run_with_all_required_signals_passes` |
| T-RP-16-002 | fixture：`gpu.device.unavailable` が 0 件 | 件数の下側 | `Err` | `Err`（specified、RP-16 の「1 件」） | 境界、陰性 | `zero_unavailable_events_is_rejected` |
| T-RP-16-003 | fixture：`gpu.device.unavailable` が 2 件 | 件数の上側 | `Err` | `Err`（specified、RP-16 の「1 件」） | 境界、陰性 | `two_unavailable_events_is_rejected` |
| T-RP-16-004 | fixture：001 と同じで、`gpu.session.created` の実際の provider が 1 件 `cuda` | GPU が無いのに cuda の session | `Err` | `Err` でその model を名指し。全件 `cpu` でなければならない（specified、RP-16 の「全件 `cpu`」） | 陰性 | `cuda_session_after_missing_gpu_is_rejected` |
| T-RP-16-005 | fixture：001 と同じで、終了 code が 2 | 止まった build | `Err` | `Err`。終了 code は 0 でなければならない（specified、RP-16） | 境界、陰性 | `nonzero_exit_code_is_rejected` |
| T-RP-16-006 | fixture：stderr に `nvidia-smi` が無い | 確かめる command | `Err` | `Err`（specified、RP-16） | 陰性 | `stderr_without_nvidia_smi_is_rejected` |
| T-RP-16-007 | fixture：`LANG=ja_JP.UTF-8` の stderr と `LANG=C.UTF-8` の stderr | 言語が別 | `Ok` | 日本語の stderr は非 ASCII の文字を 1 つ以上含み、C.UTF-8 の stderr は ASCII だけ（specified、RP-16 の「別の言語」を、この席が文字の種類で読んだ） | 陽性 | `stderr_language_follows_the_locale` |
| T-RP-16-008 | fixture：2 つの locale で同じ stderr | 切り替わらない | `Err` | `Err`（specified、RP-16） | 陰性 | `identical_stderr_across_locales_is_rejected` |
| T-RP-16-009 | fixture：GPU の在る run（終了 code 0、`final` 1 件以上、`gpu.device.unavailable` と `gpu.fallback.cpu` と `/events` の `fallback` が各 0 件、GPU の段の provider が `cuda`） | 在るのに落ちない（001 の対） | `Ok` | 別の関数 `check_gpu_run` が `Ok`（specified、RP-16 の対照） | 陽性 | `gpu_present_run_has_no_fallback_events` |
| T-RP-16-010 | fixture：009 と同じ GPU の在る run で、句読点の `gpu.fallback.cpu` が 1 件、provider が `cpu` | GPU が在るのに CPU へ落ちる build | `Err` | `check_gpu_run` が `Err` でその段を名指し（specified、RP-16 の陰性欄） | 陰性 | `gpu_present_run_that_falls_back_is_rejected` |
| T-RP-16-011 | fixture：001 と同じで、句読点の `gpu.fallback.cpu` が 0 件 | 落ちたのに log の event を出さない build | `Err` | `Err` で句読点を名指し（specified、RP-16 の陰性欄。読み込んだ段ごとに 1 件） | 陰性 | `missing_fallback_log_event_for_a_loaded_stage_is_named` |
| T-RP-16-012 | fixture：001 と同じで、`/events` の `fallback` が 0 件 | 画面へ送らない build | `Err` | `Err` で句読点を名指し（specified、RP-16、3.15 の「段ごとに出す」） | 陰性 | `missing_events_fallback_for_a_loaded_stage_is_named` |
| T-RP-16-013 | fixture：001 と同じで、句読点の `gpu.fallback.cpu` が 2 件 | 件数の上側 | `Err` | `Err` で句読点を名指し。段ごとに 1 件（specified、RP-16） | 境界、陰性 | `duplicate_fallback_event_for_a_stage_is_rejected` |
| T-RP-16-014 | fixture：001 と同じで、読み込んだ GPU の段が句読点だけ（言語判定は読み込んでいない）。言語判定の `gpu.fallback.cpu` は 0 件 | 読み込まない段 | `Ok` | `Ok`。読み込んでいない段には落ちた event を求めない（specified、RP-16 の「その run が読み込む 3.5 の GPU の段」） | 境界、陽性 | `unloaded_gpu_stage_needs_no_fallback_event` |
| T-RP-16-015 | 実走：`CUDA_VISIBLE_DEVICES` を空にして `<hy> --wav tests/golden/ja/ja_014.wav --no-realtime --mode single --lang ja` | 実走 | 実走 | 001 の形（specified、RP-16） | 陰性 | `hidden_gpu_falls_back_to_cpu_with_a_reason` |
| T-RP-16-016 | 実走：GPU の在る機械で 015 と同じ command（`CUDA_VISIBLE_DEVICES` を空にしない） | 実走の対照 | 実走 | 009 の形（specified、RP-16 の対照） | 陽性 | `visible_gpu_run_does_not_fall_back` |
| T-RP-16-017 | fixture：server に句読点の `fallback`（`to` が `cpu`）と `session_start` をこの順に送った後で、`/events` に接続した client | 遅れて接続した client | 両方を受ける | 最初に受ける event の中に `fallback` と `session_start` が 1 件ずつ在り、`fallback` が `session_start` より前。その後に送った `final` も受ける（specified、設計 3.15 の「取っておいた event を送った順に最初にまとめて送る」） | 陽性 | `a_client_that_connects_after_session_start_still_receives_it_after_the_fallbacks` |
| T-RP-16-018 | fixture：001 と同じで、読み込んだ GPU の段が句読点と言語判定。言語判定の `gpu.session.created`（`tiny-encoder.onnx`、`cpu`）、`gpu.fallback.cpu`（`stage` = `lid`）1 件、`/events` の `fallback`（`stage` = `lid`、`to` = `cpu`）1 件が足してある | 言語判定を読み込んだ run（R3 以降の落ちた形） | `Ok` | `Ok`（specified、RP-16 の「R3 以降は言語判定も」） | 陽性 | `no_gpu_run_that_also_loads_the_identifier_passes` |
| T-RP-16-019 | fixture：018 と同じで、言語判定の `gpu.fallback.cpu` が 0 件 | 落ちたのに log の event を出さない build（言語判定） | `Err` | `Err` で `lid` を名指し（specified、RP-16 の陰性欄。読み込んだ段ごとに 1 件） | 陰性 | `missing_identifier_fallback_log_event_is_named` |
| T-RP-16-020 | fixture：018 と同じで、言語判定の `/events` の `fallback` が 0 件 | 画面へ送らない build（言語判定） | `Err` | `Err` で `lid` と `/events` を名指し（specified、RP-16、3.15 の「段ごとに出す」） | 陰性 | `missing_identifier_events_fallback_is_named` |
| T-RP-16-021 | fixture：GPU の在る run（終了 code 0、`final` 1 件以上、fallback の event が 0 件）で、句読点と言語判定の `gpu.session.created` の provider が `cuda`。読み込んだ段は句読点と言語判定 | 在るのに落ちない（018 の対） | `Ok` | `check_gpu_run` が `Ok`（specified、RP-16 の対照） | 陽性 | `gpu_present_run_with_the_identifier_on_cuda_passes` |
| T-RP-16-022 | fixture：021 と同じで、言語判定の provider が `cpu` | GPU が在るのに言語判定が CPU に置かれる build | `Err` | `check_gpu_run` が `Err` で `lid` を名指し（specified、RP-16 の陰性欄） | 陰性 | `gpu_present_run_with_the_identifier_on_cpu_is_rejected` |
| T-RP-16-023 | 実走（`hayamimi-asr`、model の file が要る）：言語判定を `Gpu::Absent`（`no_device`）で開き、ja の golden の 1 本（`ja_024.wav`）を判定する | 言語判定の CPU への落とし | `Ok` | `Opened.fallback` が `stage` = `lid`、`reason` = `no_device`、`to` = `cpu` の 1 件。2 つの session の provider が全件 `cpu`。判定が `ja`（specified、3.11 の言語判定の行、3.15）。`CUDA_VISIBLE_DEVICES` を空にしても同じ | 陽性 | `lid_without_a_device_runs_on_the_cpu_and_says_so` |
| T-RP-16-024 | 実走（`hayamimi-asr`、model の file と GPU が要る）：言語判定を `Gpu::Try` で開き、ja の golden の 1 本を判定する | 言語判定の GPU の置き場 | `Ok` | `Opened.fallback` が無ければ provider が全件 `cuda`、在れば `stage` = `lid`、`to` = `cpu` で provider が全件 `cpu`。判定が `ja`（specified、3.5 の表の言語判定の行。`provider_mismatch` と `session_failed` の落としを含む） | 陽性 | `lid_placed_on_the_gpu_runs_where_it_says` |
| T-RP-16-025 | 実走（`hayamimi-asr`、model の file が要る）：ja の golden の 8 本を言語判定に掛ける | 言語判定の単独の精度 | `Ok` | 6 本以上が `ja`（derived、CPU の fp32 の実測が 6 本で、1 本が `ko`、1 本が `zh`。whisper-tiny の単独の誤りを二重確認が受ける根拠） | 境界 | `lid_names_most_golden_clips_japanese` |
| T-RP-16-026 | model の file が無い directory で言語判定を開く。`Gpu::Try` と `Gpu::Absent` の両方 | 言語判定の model が無い | `Ok` | process が終わらず、`lid` が `None`、`Opened.fallback` が `stage` = `lid`、`reason` = `missing`、`to` = `disabled`、session が 0 件（specified、3.15 の「言語判定は `--lang` の言語に固定」） | 境界、陽性 | `lid_with_no_model_is_left_out_not_a_stop` |
| T-RP-16-027 | 実走（`hayamimi-asr/tests/gpu_guard.rs`、model の file が要る）：子 process を `CUDA_VISIBLE_DEVICES` を空にして起こし、言語判定を `Gpu::Try` で開く | `Gpu::Try` を渡されたのに device が見えない | `Ok` | 子の終了 code が 0、`alive` が 1 件、`lid` の `no_device` の `cpu` への落としが 1 件（specified、3.11 の「session を作る前に device の数を数える」、3.15） | 陽性 | `hidden_gpu_with_try_sends_the_identifier_to_the_cpu_and_stays_alive` |
| T-RP-16-028 | 実走（同上）：子 process で `Engine::open`（`Settings::new` の既定は `Gpu::Try`）を `CUDA_VISIBLE_DEVICES` を空にして開く | R2 の句読点の同じ口 | `Ok` | 子の終了 code が 0、`alive` が 1 件、`punct` の `no_device` の `cpu` への落としが 1 件、provider が `cuda` の session が 0 件 | 陽性 | `hidden_gpu_with_try_sends_the_punctuation_model_to_the_cpu_and_stays_alive` |
| T-RP-16-029 | 実走（同上）：028 の対。device を隠さずに言語判定を `Gpu::Try` で開く | 在るのに落ちない | `Ok` | 子の終了 code が 0、`alive` が 1 件、落としが 0 件 | 陽性 | `visible_gpu_with_try_stays_on_the_gpu` |
| T-RP-16-030 | 実走（同上）：陰性対照。device の数を数えずに（`lid::open_as_told`）device を隠して `Gpu::Try` で開く | 数える処理を外した build | `Err` | 子の終了 code が 0 でなく、`alive` が 0 件（process が推論の library の中で終わる）。数える処理が在る T-RP-16-027 と対 | 陰性 | `hidden_gpu_without_the_check_ends_the_process` |
| T-RP-16-031 | `hayamimi-asr/tests/lid_dual.rs`：ja の golden 8 本に whisper-tiny の fp32 と int8 の判定（記録した値）。`ja_014` が `ko`、`ja_085` が `zh`、ほかは `ja` | 単独の判定 | `Ok` | 両方の build で 6 本が `ja`、8 本とも両 build の判定が同じ（derived、実測） | 境界 | `whisper_alone_names_six_of_the_eight_clips_japanese_in_both_builds` |
| T-RP-16-032 | 同上：SenseVoice の自分の判定（記録した値） | 二重確認の片方 | `Ok` | 8 本とも `<\|ja\|>`（derived、実測） | 陽性 | `sense_voice_names_all_eight_clips_japanese` |
| T-RP-16-033 | 同上：8 本それぞれを、whisper の判定と SenseVoice の判定を持つ台本の認識器と言語判定に通す（`Router::transcribe`、`balanced`） | 二重確認と粘りの後の最終の言語 | `Ok` | 8 本とも `lang` が `ja`、session の言語が `ja`（specified、設計の二重確認：session の最初の segment は SenseVoice の言語で読む） | 陽性 | `the_language_left_after_the_dual_confirmation_is_japanese_for_all_eight` |
| T-RP-16-034 | 同上：`ja` の session の次の segment で whisper が `ko` か `zh` と言い、SenseVoice は `ja` | 外れた判定が session を動かさない | `Ok` | 次の segment の `lang` が `ja` のまま（specified、二重確認：二つが一致しない切り替えは通さない） | 陰性 | `a_misnamed_clip_is_not_confirmed_as_a_switch_away_from_japanese` |
| T-RP-16-035 | 実走：本物の `<hy> --wav tests/golden/ja/ja_014.wav --no-realtime`（`--lang` を付けない） | 言語判定が実行 file に繋がっている | 実走 | 言語判定の session の `gpu.session.created` が 1 件で、確定文ごとに言語判定の結果（`final` の `lang` と、0 でない `lid_ms`）が出る（specified、設計 8 の R3 の行。持ち主は R3b） | 陽性 | `binary_run_identifies_the_language` |
| T-RP-16-036 | 実走：言語判定、認識、句読点の session を同じ process で開く test（`rp16_sessions.rs`）を `--test-threads=2` で 20 回と、abort を見た形の `cargo test --release -p hayamimi-asr --test rp16_lid -- --ignored --test-threads=2` を 20 回 | session を同時に作る形 | 0 回 | どちらも abort（`Rust cannot catch foreign exceptions`）が 0 回（specified、設計 3.4 の「session は 1 本ずつ作る」。持ち主は R3b） | 陽性 | `opening_sessions_in_parallel_never_aborts` |
| T-RP-16-037 | 実走：lock を外した build で、`rp16_lid -- --ignored --test-threads=2` を 20 回 | lock なしの対照 | 記録 | abort の回数を記録し、合否に使わない（derived。lock の根拠は、lock の無い形の 9 回で 2 回、lock を入れた後の同じ打ち方の 9 回で 0 回の実測である。abort は時刻の重なりで起きる稀な物で、lock を外した 3 つの構成の 20 回ずつでは 0 回だったので、再現しないことを lock が要らない証拠と読まない） | 記録 | `opening_sessions_without_the_lock_is_recorded` |
| T-RP-16-038 | fixture：終了 code 0、`gpu.device.unavailable` 1 件（`reason` = `no_device`）、`gpu.fallback.cpu` 1 件（`stage` = `translate`、`reason` = `no_device`、`error_code` = `HAYAMIMI_GPU_FALLBACK`）、`translate.server.started` 1 件（`gpu_layers` = 0）、server の引数の行に `-ngl 0`、stderr に `nvidia-smi` | 落ちた形が揃う | `Ok` | `Ok`（specified、RP-16） | 陽性 | `no_gpu_translate_run_with_all_required_signals_passes` |
| T-RP-16-039 | fixture：038 と同じで、`gpu.device.unavailable` が 0 件 | 件数の下側 | `Err` | `Err` で `gpu.device.unavailable` を名指し（specified、RP-16 の「1 件」） | 境界、陰性 | `translate_run_without_the_device_event_is_rejected` |
| T-RP-16-040 | fixture：038 と同じで、`gpu.fallback.cpu` が 0 件 | 落ちたのに event を出さない build | `Err` | `Err` で `translate` を名指し（specified、RP-16、3.15） | 陰性 | `translate_run_without_the_fallback_event_is_rejected` |
| T-RP-16-041 | fixture：038 と同じで、`gpu.fallback.cpu` が 2 件 | 件数の上側 | `Err` | `Err` で `translate` を名指し。段ごとに 1 件（specified、RP-16） | 境界、陰性 | `duplicate_translate_fallback_event_is_rejected` |
| T-RP-16-042 | fixture：038 と同じで、`translate.server.started` の `gpu_layers` が 99 | 在りもしない GPU に層を載せた build | `Err` | `Err` で `gpu_layers` を名指し。GPU が無いときは 0（specified、3.11 の「CPU へ落とす」） | 陰性 | `gpu_layers_after_missing_gpu_are_rejected` |
| T-RP-16-043 | fixture：038 と同じで、event は 0 のまま server の引数の行が `-ngl 99` | event と実際の引数が食い違う build | `Err` | `Err` で `-ngl` を名指し（specified。event は自己申告で、引数の行は server が受けた物） | 陰性 | `server_arguments_with_gpu_layers_after_missing_gpu_are_rejected` |
| T-RP-16-044 | fixture：038 と同じで、終了 code が 1 | 止まった start | `Err` | `Err`。終了 code は 0 でなければならない（specified、RP-16） | 境界、陰性 | `failed_translate_start_is_rejected_as_a_cpu_fallback` |
| T-RP-16-045 | fixture：038 と同じで、stderr に `nvidia-smi` が無い | 確かめる command | `Err` | `Err`（specified、RP-16） | 陰性 | `translate_stderr_without_nvidia_smi_is_rejected` |
| T-RP-16-046 | fixture：GPU の在る run（終了 code 0、`gpu.device.unavailable` と `gpu.fallback.cpu` が各 0 件、`translate.server.started` の `build` = `cuda`、`gpu_layers` = 99、引数の行に `-ngl 99`） | 在るのに落ちない（038 の対） | `Ok` | 別の関数 `check_gpu_translate_run` が `Ok`（specified、RP-16 の対照） | 陽性 | `gpu_present_translate_run_has_no_fallback_events` |
| T-RP-16-047 | fixture：046 と同じ GPU の在る run で、`gpu.fallback.cpu` が 1 件、`gpu_layers` が 0 | GPU が在るのに CPU へ落ちる build | `Err` | `check_gpu_translate_run` が `Err` で `gpu.fallback.cpu` を名指し（specified、RP-16 の陰性欄） | 陰性 | `gpu_present_translate_run_that_falls_back_is_rejected` |
| T-RP-16-048 | fixture：GPU は在るが CUDA の起動が 1 つ失敗した run（`gpu.device.unavailable` が 0 件、`translate.server.failed` 1 件、`gpu.fallback.cpu` 1 件で `reason` = `session_failed`、`gpu_layers` = 0） | 段 2 の失敗（3.11） | 理由 `session_failed` で `Ok`、理由 `no_device` で `Err` | `check_translate_on_cpu(run, session_failed)` が `Ok`。同じ run を `no_device` で確かめると `Err`（specified、3.11 の 2 段） | 境界、陽性、陰性 | `session_failed_run_needs_the_fallback_but_no_device_event` |
| T-RP-16-049 | 実走：GPU を隠し（`HIDE_GPU`）、代役の server を入れた install に `hayamimi llama start --model ok.gguf --port <空き> --dir <install> --wait 20`（`LANG` = `C.UTF-8`） | 実走 | 実走 | 038 の形。server は `/health` に `ok` を返す（specified、RP-16） | 陰性 | `hidden_gpu_translate_server_starts_on_cpu_with_a_reason` |
| T-RP-16-050 | 実走：049 と同じで、install に CUDA の build が無い（`llama/cuda` を作らない） | 起動の失敗が次の手へ落ちる | 実走 | 038 の形に加えて `translate.server.failed` が 1 件（`build` = `cuda`、`reason` = `missing_build`）で、終了 code 0（specified、3.15「止めない」） | 境界、陰性 | `missing_cuda_build_falls_through_to_the_cpu_build` |
| T-RP-16-051 | 実走、GPU の在る機械：GPU を隠さず 049 と同じ command | 実走の対照 | 実走 | 046 の形（specified、RP-16 の対照） | 陽性 | `visible_gpu_translate_server_does_not_fall_back` |

### 4.27 RP-17（自分で build した library が開け、ORT が 1 つである）

test：`native/hayamimi-gpu/tests/rp17.rs`。
`<hy> gpu-probe` の出力は、`cuda_devices`、実際の provider、`ort` が開いた ORT の版、process の中の `onnxruntime` の library の数に加え、認識器が開いている間の読込 module の一覧 `modules` を持つ。
`modules` の各行は module の名前と origin（実行 file と同じ directory なら `exe_directory`、System32 などなら `system`）を持つ（項目の綴りは実装の席が置く）。
module の名前は大文字と小文字を区別せずに数える。
CUDA の版は `modules` の名前で判定し、ORT の CUDA の provider の library の中の文字列では判定しない。
link 引数の検査は、`cargo metadata` で `hayamimi-gpu` 自身と、`hayamimi-gpu` を依存に持つ crate を数え、各 crate の bins と tests に `/DELAYLOAD:sherpa-onnx-c-api.dll` と `delayimp.lib` の 2 本が在るかを読む。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-17-001 | fixture（Windows）：`cuda_devices` 1、provider `cuda`、ORT の版 1.31.0（`build.json` の ORT の版も 1.31.0）、library 1、`onnxruntime` の origin が `exe_directory`、`modules` に `cublas64_13` と `cublasLt64_13` が各 1 で `cudart64_12` と `cublas64_12` と `cublasLt64_12` が 0 | 期待どおり | `Ok` | `Ok`（specified、RP-17） | 陽性 | `probe_output_with_all_expected_values_passes` |
| T-RP-17-002 | fixture：`cuda_devices` 0 | device 数 | `Err` | `Err`（specified、RP-17） | 陰性 | `zero_cuda_devices_is_rejected` |
| T-RP-17-003 | fixture：provider `cpu` | 配布の CPU 版に繋いだ build | `Err` | `Err`（specified、RP-17 の陰性欄） | 陰性 | `cpu_provider_is_rejected` |
| T-RP-17-004 | fixture：`build.json` の ORT の版が 1.31.0 で、`ort` が開いた ORT の版が退路の 1.28.3 | 版が違う | `Err` | `Err` で版を名指し（specified、RP-17 の「sherpa-onnx の build の ORT と同じ」。比べる相手は `build.json` の ORT の版） | 境界、陰性 | `ort_version_differing_from_build_json_is_rejected` |
| T-RP-17-005 | fixture：library の数 2 | 1 の上側 | `Err` | `Err`（specified、RP-17 の「1 つ」） | 境界、陰性 | `two_onnxruntime_libraries_are_rejected` |
| T-RP-17-006 | fixture：library の数 0 | 1 の下側 | `Err` | `Err`。0 を「重複なし」と読まない（implicit） | 境界、陰性 | `zero_onnxruntime_libraries_is_rejected` |
| T-RP-17-007 | fixture：Windows と WSL の出力 | 出力の形 | key の集合が同じ | 2 つの出力の key の集合が同じ（specified、RP-17 の陽性対照） | 陽性 | `windows_and_wsl_outputs_have_the_same_keys` |
| T-RP-17-008 | 実走：Windows と WSL で `<hy> gpu-probe` | 実走 | 実走 | 001 の形。実物の `modules` に 009 から 018 と 024 から 026 の式、`hayamimi-gpu` に依存する全 crate に 019 から 023 の式を当てて通る（specified、RP-17。WSL の build は `native/target/sherpa-gpu/linux` が指す `~/.local/share/hayamimi/build/cuda1343/linux` に在る） | 陽性 | `self_built_cuda_library_opens_one_onnxruntime` |
| T-RP-17-009 | 001 に `cudart64_12.dll` を 1 つ足した fixture | CUDA 12 の runtime が読まれた | `Err` | `Err` で `cudart64_12` を名指し（specified、RP-17 の「`cudart64_12` が 0」） | 陰性 | `cudart64_12_loaded_is_rejected` |
| T-RP-17-010 | 001 から `cublas64_13` を除いた fixture（0 件） | `cublas64_13` の下側 | `Err` | `Err` で `cublas64_13` を名指し（specified、RP-17 の「各 1」） | 境界、陰性 | `missing_cublas64_13_is_rejected` |
| T-RP-17-011 | 001 の `cublas64_13` を 2 件にした fixture | `cublas64_13` の上側 | `Err` | `Err`（specified、RP-17 の「各 1」） | 境界、陰性 | `two_cublas64_13_modules_are_rejected` |
| T-RP-17-012 | 001 から `cublasLt64_13` を除いた fixture（0 件） | `cublasLt64_13` の下側 | `Err` | `Err` で `cublasLt64_13` を名指し（specified、RP-17 の「各 1」） | 境界、陰性 | `missing_cublaslt64_13_is_rejected` |
| T-RP-17-013 | 001 に `cublas64_12.dll` を足した fixture | CUDA 12 の cublas | `Err` | `Err` で `cublas64_12` を名指し（specified、RP-17） | 陰性 | `cublas64_12_loaded_is_rejected` |
| T-RP-17-014 | 001 に `cublasLt64_12.dll` を足した fixture | CUDA 12 の cublasLt | `Err` | `Err` で `cublasLt64_12` を名指し（specified、RP-17） | 陰性 | `cublaslt64_12_loaded_is_rejected` |
| T-RP-17-015 | 001 に `cufft64_12.dll` だけを足した fixture（ほかの `_12` は 0） | `*_12` を一律 0 にしない境界 | `Ok` | `Ok`。CUDA 13.4 の toolkit 自身が `cufft64_12.dll` を持つので在ってよい（specified、RP-17） | 境界、陽性 | `cufft64_12_alone_is_accepted` |
| T-RP-17-016 | 001 の `onnxruntime` の origin を `system` にした fixture | 読み込み元 | `Err` | `Err` で origin を名指し（specified、RP-17 の陰性欄） | 陰性 | `onnxruntime_loaded_from_system_is_rejected` |
| T-RP-17-017 | 001 の module 名を `CUBLAS64_13.DLL` と `CUBLASLT64_13.DLL` にした fixture | 名前の大小 | `Ok` | `Ok`。大文字でも各 1 と数える（specified、RP-17 の「大文字と小文字を区別せずに数える」） | 境界、陽性 | `uppercase_cublas_names_are_counted` |
| T-RP-17-018 | 001 に `CUDART64_12.DLL` を足した fixture | 名前の大小（陰性側） | `Err` | `Err`。大文字の `_12` を見逃さない（specified、RP-17） | 境界、陰性 | `uppercase_cudart64_12_is_rejected` |
| T-RP-17-019 | fixture：数える crate が `hayamimi-gpu` 自身を含めて 3 つで、各 crate の bins と tests に link 引数の 2 本が在る | link 引数が全部 在る | `Ok` | `Ok`（specified、RP-17 の「全 crate の bins と tests」） | 陽性 | `all_dependent_crates_have_both_link_args_in_bins_and_tests` |
| T-RP-17-020 | fixture：1 つの crate の tests に `delayimp.lib` が無い（2 本のうち 1 本が欠ける） | 引数が 1 つ欠けた | `Err` | `Err` で crate と tests と `delayimp.lib` を名指し（specified、RP-17） | 境界、陰性 | `dependent_crate_missing_one_of_the_two_link_args_is_named` |
| T-RP-17-021 | fixture：1 つの crate が bins にだけ 2 本を持ち、tests に無い | bins の指定は tests に届かない | `Err` | `Err` で crate と tests を名指し（specified、3.4 の規則 3） | 境界、陰性 | `link_args_for_bins_do_not_cover_tests` |
| T-RP-17-022 | fixture：数える crate（`hayamimi-gpu` 自身と依存する crate）が 0 本の `cargo metadata` | 空 | `Err` | `Err`。0 本を「全 crate が満たす」と読まない（implicit。数える crate は自身を含めて 1 本以上） | 境界、陰性 | `no_counted_crates_is_an_error_not_a_pass` |
| T-RP-17-023 | 実物の `cargo metadata` と `native/*/Cargo.toml` | 数える crate の数え漏れ | 同じ集合 | metadata が出した数える crate（`hayamimi-gpu` 自身を含む）の集合が、`rg -l 'hayamimi-gpu' native/*/Cargo.toml` の集合と同じ。自身の `Cargo.toml` も名前に `hayamimi-gpu` を持つので、この集合に入る（pseudo、別の経路） | 陽性 | `counted_crates_from_metadata_equal_those_from_manifest_search` |
| T-RP-17-024 | fixture（WSL）：`modules` に `libcublas.so.13` が 1 で `libcudart.so.12` が 0 | WSL の式 | `Ok` | `Ok`（specified、RP-17。実物の照合は 031 が `~/.local/share/hayamimi/build/cuda1343/linux` の build で打つ） | 陽性 | `wsl_modules_with_cublas_13_and_without_cudart_12_pass` |
| T-RP-17-025 | 024 に `libcudart.so.12` を足した fixture | WSL の CUDA 12 | `Err` | `Err` で `libcudart.so.12` を名指し（specified、RP-17） | 陰性 | `wsl_libcudart_so_12_loaded_is_rejected` |
| T-RP-17-026 | 024 から `libcublas.so.13` を除いた fixture | WSL の cublas 13 の下側 | `Err` | `Err` で `libcublas.so.13` を名指し（specified、RP-17） | 境界、陰性 | `wsl_missing_libcublas_so_13_is_rejected` |
| T-RP-17-027 | 001 の `modules` が CUDA 13 だけで、provider の library の文字列に `cudart64_12.dll` と `cudart64_13.dll` が各 1 在る fixture | 文字列は判定に入らない | `Ok` | `Ok`。判定は `modules` の名前だけで、library の中の文字列は読まない（specified、RP-17） | 境界、陽性 | `strings_inside_the_provider_library_do_not_decide_the_cuda_version` |
| T-RP-17-028 | fixture：通った run の出力と `build.json` | `rp17_passed.json` の書き出し | 書かれる | `native/target/sherpa-gpu/<os>/rp17_passed.json` が在り、`build.json` の sha256 と probe の出力を持つ。sha256 は test が `build.json` から出した値と同じ（derived、`sha256` の関数は RB-2 の 007 で確かめた物。specified、RP-17） | 陽性 | `passing_probe_writes_rp17_passed_json_with_the_build_json_sha256` |
| T-RP-17-029 | fixture：009 の失敗した run | 落ちた run | 書かれない | `rp17_passed.json` を書かない。同じ path に前の記録が在っても、その sha256 が今の `build.json` と合わなければ RP-24 が `Err` にする（RP-24 の 014。specified、RP-17 の「通ったときは」） | 陰性 | `failing_probe_does_not_write_rp17_passed_json` |
| T-RP-17-030 | fixture：依存する crate が 0 本で、`hayamimi-gpu` 自身の tests に link 引数の 2 本が無い `cargo metadata` | 依存が 0 本でも自身を数える | `Err` | `Err` で `hayamimi-gpu` と tests を名指し。数える crate は自身の 1 本で、空とは読まない（specified、3.4 の規則 3） | 境界、陰性 | `gpu_crate_itself_is_counted_when_nothing_depends_on_it` |
| T-RP-17-031 | WSL から打つ通常の test（`#[ignore]` なし）：`native/target/sherpa-gpu/linux`（現行の `~/.local/share/hayamimi/build/cuda1343/linux` を指す symlink。`rust_port.ja.md` の 3.12 の環境の表）の先の probe の実行 file に `readelf -d` を打ち、`LD_LIBRARY_PATH` を子の process に渡さずに `<hy> gpu-probe` を起こして、認識器が開いている間の `/proc/<pid>/maps` を読む | Linux の RPATH | 通常（WSL から打つ。Windows からは symlink を読めず os error 1920 になる） | `readelf -d` の出力に `RPATH` の行が在って値が `$ORIGIN`、渡さない run で 001 の形が通る。`maps` に cuBLAS、cudart、cuDNN、ORT の行が各 1 件以上在って全部が実行 file の directory の下、`/usr/local/cuda-13.3` を含む path が 0 件。環境変数は test が読まず、子の process から外すだけである（specified、3.12 の「`$ORIGIN` の RPATH」） | 陽性 | `wsl_probe_has_origin_rpath_and_loads_every_library_from_its_own_directory` |
| T-RP-17-032 | fixture `rp17_readelf_runpath_only.txt`：`readelf -d` の出力に `RUNPATH` の行が在って値が `$ORIGIN`、`RPATH` の行は無い | RUNPATH だけでは足りない | `Err` | `check_rpath` が `Err` で RUNPATH だけだと言う。RUNPATH は実行 file が直に依る library にしか効かず、ORT の CUDA の provider が後から開く cuBLAS と cuDNN に効かないからである。031 の実物では `Ok`（specified、3.12 の「RUNPATH では足りない」） | 陰性 | `runpath_only_is_rejected_because_it_does_not_reach_libraries_opened_later` |
| T-RP-17-033 | fixture `rp17_readelf_rpath_without_origin.txt`：`RPATH` の行が在るが値が `/opt/ort` で `$ORIGIN` を含まない | RPATH の中身 | `Err` | `Err` で `$ORIGIN` が無いと言う。RPATH の行が在るだけでは通さない（derived、032 の隣の入力） | 境界、陰性 | `rpath_without_origin_is_rejected` |
| T-RP-17-034 | fixture `rp17_readelf_no_rpath.txt`：`-Wl,-rpath,$ORIGIN` を付けずに link した実行 file の `readelf -d` の出力の形で、`RPATH` も `RUNPATH` も無い | どちらも無い | `Err` | `Err` で RPATH が無いと言う。その形の実行 file は `LD_LIBRARY_PATH` が無いと起動しない。031 の現行の build（`cuda1343`）では `Ok`（specified、3.12） | 陰性 | `readelf_output_without_rpath_is_rejected` |
| T-RP-17-035 | fixture `rp17_maps_with_cuda_13_3.txt`：`maps` に cuBLAS が `/usr/local/cuda-13.3/` の下から読まれた行が 1 件在る | system の CUDA 13.3 が読まれた | `Err` | `Err` で `/usr/local/cuda-13.3` の path を名指し。031 の実物では 0 件で `Ok`（specified、3.12。R1 の実測では 13.3 の cuBLAS が読まれた） | 陰性 | `maps_with_a_cuda_13_3_path_is_rejected` |
| T-RP-17-036 | fixture `rp17_maps_no_libraries.txt`：cuBLAS、cudart、cuDNN、ORT の行が 1 件も無い `maps` | 空の読み | `Err` | `Err`。対象の library の行が 0 件の `maps` を「13.3 の path が 0 件」と読まない（implicit） | 境界、陰性 | `maps_without_any_target_library_is_an_error_not_a_pass` |

### 4.28 RP-18（句読点の同一性）

test：`native/hayamimi-asr/tests/rp18.rs`。
fixture：`mobile/hayamimi_core/test/fixtures/punct_ja_parity.json`（`cases` が 51 件。`jq '.cases \| length'` の値、2026-10-13）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-18-001 | fixture の 51 件 | 件数 | 51 | 読めた件数が 51（derived、`jq` の値）。読み取りが働く証拠 | 陽性 | `parity_fixture_has_51_cases` |
| T-RP-18-002 | fixture の全件の `input_ids` | token 化 | 全件一致 | Rust の token 化が出した `input_ids` が全件で fixture と同じ（specified、RP-18。oracle は MeCab を使わない fixture） | 陽性 | `input_ids_match_for_every_case` |
| T-RP-18-003 | fixture の 1 件目 | 陽性対照 | 一致 | 1 件目の `input_ids` が一致（specified、RP-18 の陽性対照） | 陽性 | `first_case_input_ids_match` |
| T-RP-18-004 | NFKC を掛けずに token 化した結果を fixture に掛ける | 検知の道具が差を見ること | 差が 1 件以上 | `input_ids` が fixture と違う件が 1 件以上（specified、RP-18 の陰性欄） | 陰性 | `tokenizer_without_nfkc_differs_on_some_case` |
| T-RP-18-005 | 実走：GPU で句読点の model を全件に掛ける | 実走 | 実走 | `expected` の文字列が全件一致（specified、RP-18） | 陽性 | `gpu_punctuation_matches_expected_for_every_case` |

### 4.29 RP-19（Python の test の case を Rust へ写したこと）

test：`native/hayamimi-eval/tests/rp19.rs`。
`check_port_table(table, listed_names)` は、5 の対応表の Rust の test の名前を `cargo test --release -- --list` の出力と突き合わせる。表で Rust の名前が `-` の行（写さない物）は、置き場の列に理由が在ることを確かめる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-19-001 | 5 の表の行数と、`tests/*.py` の case の数 | 全数 | 一致 | 表の行数 = 386。数は `git ls-files 'tests/*.py' \| xargs grep -c '^def test_\|^    def test_'` の合計（pseudo、2026-10-13 に打った値） | 陽性 | `table_row_count_equals_python_case_count` |
| T-RP-19-002 | fixture：表の Rust の名前が全部 list に在る | 突き合わせ | `Ok` | `Ok`（specified、RP-19） | 陽性 | `table_names_all_listed_pass` |
| T-RP-19-003 | fixture：表の 1 行目の名前（対応表の 1 行目）が list に在る | 陽性対照 | `Ok` | 1 行目の名前を含む list で `Ok`（specified、RP-19 の陽性対照） | 陽性 | `first_table_row_name_is_found` |
| T-RP-19-004 | fixture：list の名前を 1 つ変えた物 | 名前の食い違い | `Err` | `Err` でその名前を名指し（specified、RP-19 の陰性欄） | 陰性 | `renamed_test_in_list_is_named` |
| T-RP-19-005 | fixture：表の 2 行が同じ Rust の名前 | 重複 | `Err` | `Err` でその名前を名指し（derived、1 つの名前が 2 つの case を持つと、片方が消えても気付けない） | 境界、陰性 | `duplicate_rust_name_in_table_is_rejected` |
| T-RP-19-006 | fixture：Rust の名前が `-` で置き場の列が空の行 | 写さない行の理由 | `Err` | `Err` でその行の Python の case を名指し（derived、理由の無い「写さない」を許さない） | 陰性 | `unported_row_without_reason_is_rejected` |
| T-RP-19-007 | fixture：Rust の名前が `-` で置き場の列に `写さない` の理由が在る行 | 写さない行 | `Ok` | `Ok`（derived） | 境界 | `unported_row_with_reason_is_accepted` |
| T-RP-19-008 | `docs/spec/ja_pipeline_spec.json` の `config` と `<hy> config dump` から作り直した値 | 値の一致 | 差 0 | 差 0（specified、RP-19） | 陽性 | `ja_pipeline_spec_config_matches_dump` |
| T-RP-19-009 | fixture：dump の 1 つの値を変えた物 | 値の食い違い | `Err` | `Err` でその key の path を名指し（specified、RP-19） | 陰性 | `changed_config_value_is_named` |
| T-RP-19-010 | 実走：`cargo test --release -- --list` の実物と 5 の表 | 実走 | 実走 | 001 から 007 の判定を実物で通し、表の全行の Rust の test が全部通る（specified、RP-19） | 陽性 | `python_cases_are_ported` |

### 4.30 RP-20（3.5 の表の全段の置き場が決まったこと）

test：`native/hayamimi-eval/tests/rp20.rs`。
`count_measure_rows(table)` は、3.5 の表の「いまの置き場」の列に `測る` を含む行を数える。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-20-001 | fixture：「いまの置き場」が GPU、CPU、`GPU（解き直し）。判定は測る` の 3 行 | 数える道具の陽性対照 | 1 | `測る` を含む行が 1 件（specified、RP-20 の陽性対照。部分一致で数える） | 陽性 | `counter_reads_a_partial_measure_row` |
| T-RP-20-002 | 陽性対照の入力：`native/hayamimi-eval/tests/fixtures/rp20_table_before_r1.md`（R1 の前の 3.5 の表の写し。19 行） | 陽性対照 | 8 | `測る` を含む行が 8 件で、その 8 行が「int16 → float32、16 kHz への resample、mix」「VAD（Silero、sherpa-onnx の `provider`）」「冒頭欠落の疑い判定と分割再試行」（`GPU（解き直し）。判定は測る`）「ITN（漢数字 → 算用数字）」「句読点の後処理と疑問符の規則、置換辞書」「話者の速報の割り当て（cosine と重心の更新）」「清書の clustering（FastClustering）と全体の対応づけ」「照合 V と単位の換算」（derived、fixture に `awk -F'\|'` を掛けて「いまの置き場」の列だけを取り `grep -c 測る`、2026-10-14。`\| 測る \|` の完全一致では 7 件で、部分一致の 1 行を落とす。「根拠」の列にも `測る` があるので列を分ける） | 陽性 | `pre_r1_fixture_has_eight_measure_rows` |
| T-RP-20-003 | fixture：`測る` が 0 件で、全行が結果の行を持つ | 満たした表 | `Ok` | `Ok`（specified、RP-20） | 陽性 | `table_without_measure_rows_passes` |
| T-RP-20-004 | fixture：`測る` が 1 件残る表 | 境界（0 の隣） | `Err` | `Err` でその段を名指し（specified、RP-20） | 境界、陰性 | `one_remaining_measure_row_is_named` |
| T-RP-20-005 | fixture：結果の file の行を 1 つ消した物 | 対応の欠け | `Err` | `Err` でその段を名指し（specified、RP-20 の陰性欄） | 陰性 | `stage_without_a_result_row_is_named` |
| T-RP-20-006 | fixture：表 19 行に対し結果の行が 19 | 対応の数 | `Ok` | 全件に対応（specified、RP-20 の「全件」） | 陽性 | `all_nineteen_rows_have_results_pass` |
| T-RP-20-007 | 3.5 の表と `gpu_stages.md` の実物（docs だけを読む。testdata も GPU も要らない） | 実物の読み取り（`--ignored` 無し） | 全行 | 003 の形（specified、RP-20） | 陽性 | `every_stage_placement_is_measured` |
| T-RP-20-008 | 設計の 3.5 の表の実物（19 行。`### 3.5` から `### 3.6` の間の `^\| ` の行から見出しの 1 行を引いた値） | 満たした表。0 は読み取りの失敗と同じ顔をするので、002 の 8 件と、同じ列の `CPU` を含む行が 1 以上であることを対照にする | 0 | `測る` を含む行が 0 件（derived、実物に `awk -F'\|'` で「いまの置き場」の列だけを取り `grep -c 測る`、2026-10-14。同じ列の `grep -c CPU` は 15）。R1 の結果が無くても打てる | 陽性 | `current_design_table_has_no_measure_rows` |
| T-RP-20-009 | fixture：「いまの置き場」が `CPU`、「根拠、または R1 で測る物」が `R5 が CPU の p50 を測る` の 1 行 | 列の取り違え | 0 | `測る` を含む行が 0 件。「根拠」の列の語を数えない（specified、RP-20 の「いまの置き場」の列。設計の実物の根拠の列には `測る` が 7 行に在る） | 境界、陰性 | `measure_word_in_evidence_column_is_not_counted` |

### 4.31 RP-21（model の取得に Python が要らない）

test：`native/hayamimi/tests/rp21.rs`。001、005、006、007、012、013 は実走で、R10 が置く。007 が使う RP-9 の `check_descendants` も R10 が作る。
`<tmp>` は `native/target/` の下に test が作る一時の directory（`/tmp` は使わない）。
取る版は 3.5 の置き場で決まる（3.10）。3.5 の表で CPU とした認識（ReazonSpeech）は int8、GPU とした言語判定（whisper-tiny）と句読点の BERT は fp32 か fp16 である（3.4）。
版は file 名で見分ける。int8 は `.int8.onnx` で終わり、fp32 は suffix を持たず、fp16 は `.fp16.onnx` で終わる（`models/sherpa-onnx-whisper-tiny/` の `tiny-encoder.int8.onnx` と `tiny-encoder.onnx`、`gpu_stages.md` の node の表の `.fp16.onnx` による。ReazonSpeech の tarball の file 名は NOT VERIFIED。実機の tarball の一覧で確かめる）。
case は 4 組（ReazonSpeech、whisper-tiny、Silero、句読点）の存在と、manifest の項目の file 名で書く。Silero は精度の版を持たない 1 file（`silero_vad.onnx`）なので、版の case に入れない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-21-001 | 実走：空の directory に `<hy> models fetch --minimal --dir <tmp>` | 4 組の取得 | 実走 | 4 組が揃い、`models/manifest.json` が在る。認識の encoder の項目が `.int8.onnx` で終わり、言語判定と句読点の項目が `.int8.onnx` で終わらない（specified、RP-21 の「3.5 の置き場に合った版」と 3.10） | 陽性 | `minimal_fetch_gets_the_four_groups_and_a_manifest` |
| T-RP-21-002 | manifest の各 file の項目 | sha256 と bytes | 全 file が持つ | 各 file が 64 桁の 16 進の sha256 と 0 より大きい bytes を持つ（specified、RP-21） | 陽性 | `manifest_entries_have_sha256_and_bytes` |
| T-RP-21-003 | fixture：sha256 が 63 桁の項目 | 桁の下側 | `Err` | `Err` でその file を名指し（derived、64 桁でない） | 境界、陰性 | `sha256_of_63_hex_digits_is_rejected` |
| T-RP-21-004 | fixture：bytes が 0 の項目 | bytes の境界 | `Err` | `Err`（derived、0 より大きい） | 境界、陰性 | `zero_bytes_entry_is_rejected` |
| T-RP-21-005 | 実走：002 の directory で同じ command をもう一度 | 取り直さない | 実走 | 各 file の更新時刻が変わらない（specified、RP-21 の陽性対照） | 陽性 | `second_fetch_does_not_refetch` |
| T-RP-21-006 | 実走：取得先を 2 つとも壊した 2 回目の取得（取得先を指す口は実装の席が置く） | 取得の失敗 | 実走 | 終了 code が 0 で、既に在る file の bytes が前と同じで、directory に増えた file が 0、`models.fallback.previous` が 1 件（specified、RP-21 の陰性欄と 3.10 の「前の file を残し、一時 file に書いてから rename」） | 陰性 | `broken_sources_keep_the_previous_files_and_say_so` |
| T-RP-21-007 | 実走：実行中の子孫の process | Python が要らない | 実走 | 取得中の子孫に `python` を含む名前が 0（specified、RP-21 の題。RP-9 の `check_descendants` を使う） | 陽性 | `fetch_runs_without_python` |
| T-RP-21-008 | fixture：manifest の項目が 認識の encoder `recognizer-encoder.int8.onnx`、言語判定 `tiny-encoder.onnx` と `tiny-decoder.onnx`、句読点 `punct_bert.onnx`、Silero `silero_vad.onnx`（file 名は fixture の仮の名前）。3.5 の表は認識が CPU（int8）、言語判定と句読点が GPU | 版が置き場に合う | `Ok` | `Ok`（specified、RP-21 の「3.5 の置き場に合った版」。認識は int8、GPU の段は int8 でない） | 陽性 | `manifest_versions_match_the_stage_placements` |
| T-RP-21-009 | fixture：008 の認識の encoder の項目を `recognizer-encoder.onnx`（fp32）に替えた物 | 認識が int8 でない | `Err` | `Err` で認識の encoder を名指し（specified、RP-21 と 3.10 の「認識は int8」） | 陰性 | `fp32_recognizer_in_the_manifest_is_rejected` |
| T-RP-21-010 | fixture：008 の言語判定の項目を `tiny-encoder.int8.onnx` に替えた物 | GPU の段が int8 | `Err` | `Err` で言語判定の encoder を名指し。int8 の op は CUDA に無い（specified、3.4 と RP-21。S6） | 陰性 | `int8_model_for_a_gpu_stage_is_rejected` |
| T-RP-21-011 | fixture：008 の句読点の項目を `punct_bert.fp16.onnx` に替えた物 | GPU の段の fp16 | `Ok` | `Ok`（specified、3.4 の「fp32 か fp16」。008 は fp32 の側、これは fp16 の側） | 境界、陽性 | `fp16_model_for_a_gpu_stage_passes` |
| T-RP-21-012 | 実走：空の directory で、取得先の 1 つ（sherpa-onnx の release）だけを壊して `<hy> models fetch --minimal --dir <tmp>` | 取得先の片方の失敗 | 実走 | 終了 code が 0 で、4 組が揃い、`models/manifest.json` が在り、もう 1 つの取得先から取れた file の sha256 が manifest の項目と一致する（specified、RP-21 の陰性欄） | 陰性 | `one_broken_source_falls_back_to_the_other_source` |
| T-RP-21-013 | 実走：002 の directory で、取得先が両方在る 2 回目の取得 | 在るのに落ちない（006 の対） | 実走 | `models.fallback.previous` が 0 件で、終了 code が 0（specified、RP-21 の対照） | 陽性 | `healthy_sources_emit_no_previous_files_event` |

### 4.32 RP-22（GPU の memory の上限）

test：`native/hayamimi-eval/tests/rp22.rs`。
`check_vram(samples)` は、`nvidia-smi --query-gpu=memory.used --format=csv` の 1 秒ごとの読みの最大が 14,000 MiB 以下であることを確かめる（14,000 は仮の値。`gpu_stages.md` に MiB の測定が無いので、R3 と R8e が RP-3 の run で測って置き直す）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-22-001 | fixture：最大 14000 MiB | 上限の等号 | `Ok` | `Ok`（specified、RP-22） | 境界、陽性 | `peak_of_14000_mib_passes` |
| T-RP-22-002 | fixture：最大 14001 MiB | 上限の外側 | `Err` | `Err`（specified、RP-22） | 境界、陰性 | `peak_of_14001_mib_is_rejected` |
| T-RP-22-003 | fixture：読みが 0 個 | 空 | `Err` | `Err`。0 個を「上限以下」と読まない（implicit） | 境界、陰性 | `no_samples_is_an_error` |
| T-RP-22-004 | fixture：run の前の読み（起動前）と run の間の読み | 陽性対照 | 前の読みが取れる | 起動前の値が 1 つ以上読める（specified、RP-22 の陽性対照） | 陽性 | `pre_start_reading_is_available` |
| T-RP-22-005 | fixture：`MiB` の単位が付いた行と付かない行 | 単位の読み | 数が読める | `1161 MiB` から 1161 が読める。単位の無い行は `Err`（implicit） | 境界 | `unit_suffix_is_parsed_and_missing_unit_is_rejected` |
| T-RP-22-006 | fixture：最大 16000 MiB の読み | 上限を超えた読み | `Err` | `Err` で 16000 の読みを名指し（specified、RP-22 の陰性欄） | 陰性 | `reading_over_the_budget_is_rejected` |
| T-RP-22-007 | 実走：R3b は llama-server を起こした機械で `<hy> --wav testdata/lat-multi.wav` を実時間で流す間の読み、R8e は RP-3 の run の間の読み | 実走 | 実走 | 最大が 14,000 MiB 以下（specified、RP-22。読む物は GPU 全体の `memory.used` で、llama-server を含む。設計 8 の「07:00 の計画」の R3 の着手の 2 と 3） | 陽性 | `vram_stays_under_budget_during_the_run` |
| T-RP-22-010 | 実走：`<hy> --wav testdata/lat-multi.wav --max-resident 2` | CPU に常駐させる認識器の数の上限 | 実走 | ReazonSpeech の外に常駐した認識器の数が、どの時点でも 2 以下で、追い出しの event が 1 件以上（specified、設計 8 の「07:00 の計画」の R3 の着手の 3。5 言語で使う認識器は ReazonSpeech の外に 3 つなので、2 では追い出しが起きる。event の名前は実装の席が置く） | 陽性 | `least_recently_used_recognizer_is_dropped_at_the_cap` |
| T-RP-22-011 | 実走：010 と同じ音声を `--max-resident 3`（既定）で | 上限に届かない数（010 の対） | 実走 | 追い出しの event が 0 件（specified、同上。3 つは上限の 3 に収まる） | 陽性 | `default_cap_drops_nothing_for_five_languages` |
| T-RP-22-008 | 実走：起動前の読みを 3 回取り、その最大より 1 MiB 高い値を上限に置いて、`<hy> --wav testdata/lat-multi.wav` を実時間で起こす（007 の run の最大は使わない。読む物は GPU 全体で、他の process の使用量が run ごとに動くので、別の run の最大は、この run が届かない所に在りうる） | 上限に届いた run（落ちた形） | 実走 | `hayamimi` が止まらず（終了 code 0、`final` が出る）、`gpu.fallback.cpu`（`reason` = `memory_budget`）が 1 件以上、`/events` の `fallback` が同じ数。`check_vram` は上限を超えた読みを名指しして `Err`（specified、RP-22 の陰性欄と 3.15 の memory の行） | 陰性 | `budget_reached_falls_back_instead_of_stopping` |
| T-RP-22-009 | 実走：007 と同じ run（上限 14,000 MiB） | 届かない run（007 と 008 の対） | 実走 | `gpu.fallback.cpu`（`reason` = `memory_budget`）が 0 件（specified、RP-22。上限に届かなければ落ちない） | 陽性 | `budget_not_reached_emits_no_memory_fallback` |

### 4.33 RP-23（段ごとの GPU と CPU の計測）

test：`native/hayamimi-eval/tests/rp23.rs`。
`check_stage_results(table, results)` は、`docs/results/gpu_stages.md` が 3.5 の表で GPU の経路が在る段（「いまの置き場」が「CPU（GPU 経路なし）」で始まらない 14 段）のそれぞれについて、無負荷と負荷の下の、GPU と CPU の p50 と p95 を持つことを確かめる。
GPU の経路が無い 5 段は、持ち主の slice が測るまで結果の行を求めない（RP-8 の根拠 4 が見る）。
あわせて、JIT の cache が空の run と温かい run の `decode_ms` が別の行で在ることを確かめる（10 の H27）。
cache が空の run は、子の process に空の directory を `CUDA_CACHE_PATH` で渡して起こす。test はこの値を読まず、子へ渡すだけである。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-23-001 | fixture：GPU の経路が在る 14 段 × {無負荷、負荷} × {GPU、CPU} の全部に p50 と p95 が在り（GPU の経路が無い 5 段は未測定）、JIT の cache が空の run と温かい run の `decode_ms` の行が在る結果 | 満たした結果 | `Ok` | `Ok`（specified、RP-23） | 陽性 | `complete_results_pass` |
| T-RP-23-002 | fixture：1 段の「負荷の下の GPU」が無い | 組の欠け | `Err` | `Err` でその段と組を名指し（specified、RP-23） | 陰性 | `missing_cell_is_named` |
| T-RP-23-003 | fixture：p95 が無く p50 だけの行 | 項目の欠け | `Err` | `Err`（specified、RP-23 の「p50 と p95」） | 陰性 | `row_without_p95_is_rejected` |
| T-RP-23-004 | fixture：標本の数が「lat-ja の発話の数 × `--repeat`（23 × 5）」より 1 少ない行 | 標本数の下側 | `Err` | `Err` で期待の数と行の数を名指し。期待の数は manifest の行数 × 繰り返しの数で、定数に置かない（specified、RP-23 の「lat-ja の 23 発話を 5 回流した区切りの数」） | 境界、陰性 | `sample_count_one_short_of_utterances_times_repeats_is_rejected` |
| T-RP-23-005 | fixture：標本の数が 23 × 5 の行 | 標本数の等号 | `Ok` | `Ok`（specified、RP-23） | 境界、陽性 | `sample_count_of_utterances_times_repeats_is_accepted` |
| T-RP-23-006 | fixture：provider の項目が無い行 | provider を指定しない run | `Err` | `Err`。GPU の行として受け付けない（specified、RP-23 の陰性欄） | 陰性 | `row_without_provider_is_not_a_gpu_row` |
| T-RP-23-007 | fixture：同じ build で同じ入力を 2 回流した p50 が 1000 µs と 1100 µs | 10% の等号 | `Ok` | `Ok`。式：差 100 ÷ 1回目 1000 = 10%（specified、RP-23 の陽性対照。基準は 1 回目の値と読んだ） | 境界、陽性 | `repeat_p50_difference_of_ten_percent_passes` |
| T-RP-23-008 | fixture：1000 µs と 1101 µs | 10% の外側 | `Err` | `Err`（specified、RP-23） | 境界、陰性 | `repeat_p50_difference_over_ten_percent_is_rejected` |
| T-RP-23-009 | fixture：1 発話あたりの転送の時間と bytes の項目が無い結果 | 転送の項目 | `Err` | `Err` で項目を名指し（specified、RP-23） | 陰性 | `result_without_transfer_fields_is_rejected` |
| T-RP-23-010 | fixture：測った機械と `build.json` の要点の節が無い結果 | 環境の項目 | `Err` | `Err`（specified、RP-23） | 陰性 | `result_without_machine_and_build_summary_is_rejected` |
| T-RP-23-011 | 実走：`<ev> stages --provider cuda` と `--provider cpu` を無負荷と負荷の下で。`cuda` は JIT の cache が空の run と、同じ cache で続けた温かい run の 2 回 | 実走 | 実走 | 結果の file を書いて 001 の形で通る（specified、RP-23） | 陽性 | `every_stage_is_timed_on_gpu_and_cpu` |
| T-RP-23-012 | fixture：JIT の cache が空の run の `decode_ms` の行が無い結果（温かい run の行は在る） | 空の run の行の欠け | `Err` | `Err` で cache が空の run の行を名指し（specified、RP-23 の「JIT の cache が空の run と温かい run の `decode_ms` が別の行で在る」） | 陰性 | `result_without_empty_jit_cache_decode_row_is_rejected` |
| T-RP-23-013 | fixture：温かい run の `decode_ms` の行が無い結果（空の run の行は在る） | 温かい run の行の欠け | `Err` | `Err` で温かい run の行を名指し（specified、RP-23） | 陰性 | `result_without_warm_jit_cache_decode_row_is_rejected` |
| T-RP-23-014 | fixture：2 つの run の `decode_ms` が 1 行にまとめられ、cache の状態の項目が無い結果 | 別の行ではない | `Err` | `Err`。2 行に分かれていないものを通さない（specified、RP-23 の「別の行」） | 境界、陰性 | `merged_cache_state_decode_rows_are_rejected` |
| T-RP-23-015 | 設計の 3.5 の表の実物 | 数える段の読み取り | 14 | 「いまの置き場」が「CPU（GPU 経路なし）」で始まらない行が 14、始まる行が 5（取り込み、ITN、句読点の後処理、清書の clustering、配信）。14 行の置き場の内訳は、GPU が 3（言語判定、句読点、訳 T）、CPU が 10（resample、ring buffer、VAD、認識、冒頭欠落の判定、話者の埋め込み、話者の速報の割り当て、清書の再分離、認識の文字列 → 訳の入力、照合 V）、host が 1（fbank と探索）。置き場が CPU でも GPU の経路が在る段は 14 に入る（derived、`awk -F'\|'` で列を取り数えた値、2026-10-14） | 陽性 | `stages_with_a_gpu_path_are_fourteen` |
| T-RP-23-016 | fixture：001 から、GPU の経路が在る 1 段（句読点の BERT）の 4 組を消した結果 | 14 段の欠け | `Err` | `Err` でその段を名指し。GPU の経路が無い 5 段と同じ扱いにしない（specified、RP-23） | 陰性 | `missing_stage_with_a_gpu_path_is_named` |
| T-RP-23-017 | fixture：001 から、置き場が CPU（int8）の認識の段の 4 組を消した結果 | 置き場が CPU でも GPU の経路が在る段 | `Err` | `Err` で認識の段を名指し。「いまの置き場」が `CPU` で始まることを「GPU の経路なし」と読まない（specified、RP-23。「CPU（GPU 経路なし）」で始まらない 14 段）。`CPU` の前方一致で 5 段を決める実装は、認識の段を 14 から落として通すので、この case で落ちる | 境界、陰性 | `cpu_placed_stage_with_a_gpu_path_still_requires_results` |

### 4.34 RP-24（build の再現）

test：`native/xtask/tests/rp24.rs`。
`compare_build_json(a, b)` は、2 つの `build.json` の 5 項目（sherpa-onnx の commit、ORT の版、CUDA の主版、cuDNN の主版、`sm_120` 系の判定）を比べる。CUDA の副版は記録だけで、一致を求めない（3.12）。
`sm_120` 系の判定の入力は、`build.json` の `sm_120` と `sm_120a` の機械語の件数、`sm_120` の PTX の件数、`rp17_passed.json` である。
判定の式は RP-24 の「各 OS で、`sm_120` か `sm_120a` の機械語が 1 件以上、または `sm_120` の PTX があり「PTX の JIT で RP-17 が通った記録」が在る。この記録は、その build の `build.json` の sha256 を持つ `rp17_passed.json`（RP-17）である」である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-24-001 | fixture：5 項目が同じ 2 つの `build.json` | 一致 | `Ok` | `Ok`（specified、RP-24） | 陽性 | `identical_five_fields_pass` |
| T-RP-24-002 | fixture：ORT の版だけ 1.31.0 と退路の 1.28.3 | ORT の版 | `Err` | `Err` で ORT の版を名指し（specified、RP-24 の陰性欄） | 陰性 | `ort_version_difference_is_named` |
| T-RP-24-003 | fixture：sherpa-onnx の commit だけ違う | commit | `Err` | `Err` で commit を名指し（specified、RP-24） | 陰性 | `sherpa_commit_difference_is_named` |
| T-RP-24-004 | fixture：CUDA の主版が 12 と 13 | CUDA の主版 | `Err` | `Err`（specified、RP-24） | 陰性 | `cuda_major_difference_is_named` |
| T-RP-24-005 | fixture：cuDNN の主版が 9 と 8 | cuDNN の主版 | `Err` | `Err`（specified、RP-24） | 陰性 | `cudnn_major_difference_is_named` |
| T-RP-24-006 | fixture：`sm_120` 系の判定が true と false | 判定の食い違い | `Err` | `Err` で判定を名指し（specified、RP-24） | 陰性 | `sm_120_family_verdict_difference_is_named` |
| T-RP-24-007 | fixture：CUDA の副版だけ 12.8 と 12.9 | 副版は求めない | `Ok` | `Ok`（specified、3.12 の「副版は記録し、一致を求めない」） | 境界、陽性 | `cuda_minor_difference_is_accepted` |
| T-RP-24-008 | fixture：`sm_120` と `sm_120a` の機械語が各 0 件、`sm_120` の PTX が在り、`rp17_passed.json` が無い | 機械語の件数と記録 | `Err` | `Err`（specified、RP-24 の「機械語が 1 件以上、または PTX があり RP-17 が通った記録が在る」の両方が偽） | 境界、陰性 | `zero_sm_120_family_elf_without_rp17_record_is_rejected` |
| T-RP-24-009 | fixture：機械語が 0 件、`sm_120` の PTX が在り、`rp17_passed.json` が在って sha256 が `build.json` と同じ | PTX の JIT | `Ok` | `Ok`（specified、RP-24） | 陽性 | `zero_sm_120_family_elf_with_ptx_and_rp17_record_is_accepted` |
| T-RP-24-010 | fixture：`sm_120` の機械語が 1 件 | 件数の下端 | `Ok` | `Ok`（specified、RP-24） | 境界、陽性 | `one_sm_120_elf_is_accepted` |
| T-RP-24-011 | 実走：同じ OS で 2 回 build した `build.json` | 陽性対照 | 実走 | 5 項目が一致（specified、RP-24 の陽性対照） | 陽性 | `two_builds_on_one_os_match` |
| T-RP-24-012 | 実走：Windows と WSL で `cargo run -p xtask -- sherpa-gpu`。2 つの `build.json` は WSL から打つ test が読む（`sherpa-gpu/linux` の symlink を Windows から読むと os error 1920 になる） | 実走 | 実走（WSL から打つ） | 5 項目が一致（specified、RP-24） | 陽性 | `windows_and_wsl_builds_match` |
| T-RP-24-013 | fixture：`sm_120a` の機械語が 1 件、`sm_120` の機械語と PTX が 0 件、`rp17_passed.json` が無い | `sm_120a` だけの ELF | `Ok` | `Ok`。`sm_120a` の機械語は `sm_120` 系の機械語に数える（specified、RP-24 の「`sm_120` か `sm_120a` の機械語が 1 件以上」） | 境界、陽性 | `sm_120a_only_elf_is_accepted` |
| T-RP-24-014 | fixture：機械語が 0 件、PTX が在り、`rp17_passed.json` の sha256 が今の `build.json` と違う | 別の build の記録 | `Err` | `Err` で sha256 の食い違いを名指し。記録は「その build の `build.json` の sha256」を持つ物だけが数に入る（specified、RP-24） | 境界、陰性 | `rp17_record_for_another_build_is_rejected` |
| T-RP-24-015 | fixture：機械語が 0 件、`sm_120` の PTX が 0 件、`rp17_passed.json` が在って sha256 が `build.json` と同じ | PTX が無い | `Err` | `Err`。PTX が無ければ JIT で動いた記録を数えない（specified、RP-24 の「`sm_120` の PTX があり」） | 境界、陰性 | `rp17_record_without_sm_120_ptx_is_rejected` |
| T-RP-24-016 | fixture：`sm_90a` と `sm_100a` の機械語だけ在り、`sm_120` 系の機械語と PTX が 0 件、記録が無い | 別の arch の機械語 | `Err` | `Err`。`sm_120` 系以外の機械語は数えない（specified、RP-24） | 境界、陰性 | `other_arch_elf_does_not_count_as_sm_120_family` |

### 4.35 RP-25（訳の揺れ）

test：`native/hayamimi-eval/tests/rp25.rs`。
入力は RP-3 の Rust の run の event 列。`check_flicker(events)` は、発話ごとに表示の訳の列と、外へ送った訳の列を確かめる。
種類は 先の訳（`stage` = `t`）、確定の訳（`stage` = `e` か、E が V に落ちて残った T）、速報の訳である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-25-001 | fixture：先の訳 → 確定の訳（別の文字列）→ 以後の変化なし | 置き換え 1 回 | `Ok` | `Ok`（specified、RP-25 の「1 回まで」） | 境界、陽性 | `one_replacement_from_early_to_final_passes` |
| T-RP-25-002 | fixture：先の訳 → 別の先の訳 → 確定の訳（置き換えが 2 回） | 置き換えの上側 | `Err` | `Err`（specified、RP-25） | 境界、陰性 | `two_replacements_before_final_are_rejected` |
| T-RP-25-003 | fixture：確定の訳の後に表示の訳が変わる | 確定の後の変化 | `Err` | `Err` で発話の `id` を名指し（specified、RP-25 の「0」） | 陰性 | `change_after_final_is_rejected` |
| T-RP-25-004 | fixture：速報の訳で確定した字形の接頭辞が後で消える | 接頭辞の消失 | `Err` | `Err`（specified、RP-25 の「0」） | 陰性 | `vanished_committed_prefix_is_rejected` |
| T-RP-25-005 | fixture：Zoom へ同じ `id` の訳が 1 件、Teams と SRT も 1 件ずつで、全部が確定の訳と同じ文字列 | 外へ出た訳 | `Ok` | `Ok`（specified、RP-25） | 陽性 | `one_send_per_id_equal_to_final_passes` |
| T-RP-25-006 | fixture：Zoom へ同じ `id` を 2 件 | 送信の数の上側 | `Err` | `Err`（specified、RP-25 の「1 件」） | 境界、陰性 | `two_sends_for_one_id_are_rejected` |
| T-RP-25-007 | fixture：先の訳を Zoom へ送った後に確定の訳が違う文字列 | 先の訳を外へ送る偽の中継 | `Err` | `Err`（specified、RP-25 の陰性欄。送った字幕は置き換えられない） | 陰性 | `early_translation_sent_outside_is_rejected` |
| T-RP-25-008 | fixture：先の訳と確定の訳が違う発話が 2 つ | 数える道具 | 2 | 数 = 2（specified、RP-25 の陽性対照） | 陽性 | `counter_reads_two_utterances_with_different_early_and_final` |
| T-RP-25-009 | fixture：E が V に落ちて T の訳が確定の訳になる発話 | 確定の決め方 | `Ok` | `Ok`。置き換えが 0 回（specified、3.6 の 2） | 境界 | `held_e_keeps_the_early_translation_as_final` |
| T-RP-25-010 | 実走：RP-3 の Rust の run を読む | 実走 | 実走 | 全発話で 001 から 009 の判定を通る（specified、RP-25） | 陽性 | `rp25_translation_flicker_is_bounded` |

### 4.36 RP-26（言語の切り替え）

test：`native/hayamimi/tests/rp26.rs`。
`check_locale_output(en, ja, tables)` は、`--help` と fallback の時の stderr（3.15）を、2 つの locale で読んだ結果を確かめる。
locale の決め方は、`ja_JP.UTF-8` を ja、`C.UTF-8` と未知の値を en とする（3.14 の「既定は英語」）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-26-001 | fixture：`strings.en.toml` と `strings.ja.toml` が同じ key の集合 | key の集合 | `Ok` | `Ok`（specified、RP-26） | 陽性 | `identical_key_sets_pass` |
| T-RP-26-002 | fixture：ja の表に key が 1 つ無い | key の欠け | `Err` | `Err` でその key を名指し（specified、RP-26） | 陰性 | `missing_key_in_ja_is_named` |
| T-RP-26-003 | fixture：en の表に key が 1 つ多い | key の余り | `Err` | `Err` でその key を名指し（specified、RP-26） | 陰性 | `extra_key_in_en_is_named` |
| T-RP-26-004 | fixture：en の出力の全行が表の値から組まれ、ja の出力の全行も表の値から組まれている | 出力の出所 | `Ok` | `Ok`（specified、RP-26） | 陽性 | `output_built_only_from_table_values_passes` |
| T-RP-26-005 | fixture：en の出力に表に無い 1 行 | 文言の直書き | `Err` | `Err` でその行を名指し（specified、RP-26 の陰性欄） | 陰性 | `hardcoded_line_not_in_the_table_is_named` |
| T-RP-26-006 | fixture：en の出力は ASCII だけ、ja の出力は非 ASCII を含む | 言語が別 | `Ok` | `Ok`（specified、RP-26 の「別の言語」を、この席が文字の種類で読んだ） | 陽性 | `en_is_ascii_and_ja_has_non_ascii_text` |
| T-RP-26-007 | fixture：2 つの locale で同じ出力 | 切り替わらない | `Err` | `Err`（specified、RP-26） | 陰性 | `identical_output_for_both_locales_is_rejected` |
| T-RP-26-008 | locale の値 `ja_JP.UTF-8`、`C.UTF-8`、`fr_FR.UTF-8` | locale の決め方 | ja、en、en | 決める関数が ja、en、en を返す（specified、3.14 の「既定は英語」） | 境界 | `locale_resolution_defaults_to_english` |
| T-RP-26-009 | 実走：`<hy> --help` と、`CUDA_VISIBLE_DEVICES` を空にして fallback の stderr を出す run を `LANG` を en と ja にして起こす | 実走 | 実走 | 001、004、006 の形（specified、RP-26）。`LANG` は test が子の process に渡す値で、test の code は読まない | 陽性 | `messages_follow_the_locale` |
| T-RP-26-010 | fixture：GPU を隠した run の fallback の stderr。en と ja の全行が表の値から組まれている | fallback の文の出所 | `Ok` | `Ok`（specified、RP-26 と 3.15 の「利用者の locale の文で 1 回」） | 陽性 | `fallback_stderr_built_only_from_table_values_passes` |
| T-RP-26-011 | fixture：en の fallback の stderr に表に無い 1 行 | fallback の文言の直書き | `Err` | `Err` でその行を名指し（specified、RP-26 の陰性欄） | 陰性 | `hardcoded_fallback_line_is_named` |

### 4.37 RP-27（GPU の経路が無い文字列の段が ORT と CUDA を呼ばない）

test：`native/hayamimi-asr/tests/rp27.rs`。受入 test の名前は `rp27_string_stages_do_not_import_ort_or_cuda`。R2 が作る crate `native/hayamimi-asr` の test で、006 は R2 の木で `itn.rs` と `punct/rules.rs` が在り rc 1 になる。
`count_gpu_imports(files)` は、`rg -c -i '\bort\b\|cuda' <files>` を子の process で打ち、終了 code と当たりの数を返す。
終了 code は 0（当たりが在る）、1（0 件）、2（file が無いなどの error）で、2 を 0 件と読まない。
fixture の木は `native/target/` の下に test が作る一時の directory に置く（`/tmp` は使わない）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-27-001 | fixture：`ort` も `cuda` も含まない `itn.rs` と `punct/rules.rs` の木 | 0 件 | rc 1 | rc が 1 で当たりが 0 件（specified、RP-27） | 陽性 | `clean_string_stage_files_have_no_hits` |
| T-RP-27-002 | fixture：001 の `itn.rs` に `use ort` の 1 行を足した木 | 1 件（0 の隣） | rc 0 | rc が 0 で当たりが 1 件。判定は `Err` で `itn.rs` を名指し（specified、RP-27 の陰性欄） | 境界、陰性 | `ort_import_in_itn_is_named` |
| T-RP-27-003 | fixture：001 の `punct/rules.rs` に `CUDA` の語（大文字）を含む 1 行を足した木 | 大文字小文字 | rc 0 | 当たりが 1 件。`-i` で大文字を拾い、`Err` で `punct/rules.rs` を名指し（specified、RP-27 の式の `-i`） | 境界、陰性 | `uppercase_cuda_in_punct_rules_is_named` |
| T-RP-27-004 | fixture：`punct/rules.rs` が無い木 | file の欠け | rc 2 | `Err`。rc 2 を 0 件と読まない（specified、RP-27 の「file が無いときの rc 2 は落ちる」） | 境界、陰性 | `missing_string_stage_file_is_an_error_not_zero_hits` |
| T-RP-27-005 | 実走：`native/hayamimi-gpu/src/` に同じ式 | 数える式の陽性対照 | rc 0 | 当たりが 1 件以上（specified、RP-27 の陽性対照） | 陽性 | `import_counter_finds_ort_and_cuda_in_the_gpu_crate` |
| T-RP-27-006 | 実走（R2 の後）：`native/hayamimi-asr/src/itn.rs` と `native/hayamimi-asr/src/punct/rules.rs` | 実物 | rc 1 | 2 file が在り、rc が 1 で当たりが 0 件（specified、RP-27） | 陽性 | `string_stage_modules_import_neither_ort_nor_cuda` |

### 4.38 RP-28（CPU に落ちた時の主指標 `L`）

test：`native/hayamimi-eval/tests/rp28.rs`。受入 test の名前は `rp28_cpu_fallback_latency_is_recorded_apart`。
`check_cpu_run_latency(report, run)` は、CPU に落ちた run の `L`、`L_asr`、`L_tr` の p50 と p95 が GPU の run の行（RP-3）とは別の行に在ることと、仮の上限（`L` の p50 が 4500 ms、p95 が 7000 ms）以下であることを確かめる。run は `gpu.fallback.cpu` の件数を持つ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-28-001 | fixture：`gpu.fallback.cpu` が 1 件以上の run。別の行に `L` の p50 が 4500 ms、p95 が 7000 ms、`L_asr` と `L_tr` の p50 と p95 | 仮の上限の等号 | `Ok` | `Ok`。式：4500 ≤ 4500、7000 ≤ 7000（specified、RP-28。上限は仮の値で R8e が置き直す） | 境界、陽性 | `cpu_run_at_the_provisional_limits_passes` |
| T-RP-28-002 | fixture：001 で `L` の p50 が 4501 ms | p50 の外側 | `Err` | `Err` で `L` の p50 を名指し（specified、RP-28） | 境界、陰性 | `cpu_run_p50_over_the_limit_is_rejected` |
| T-RP-28-003 | fixture：001 で `L` の p95 が 7001 ms | p95 の外側 | `Err` | `Err` で `L` の p95 を名指し（specified、RP-28） | 境界、陰性 | `cpu_run_p95_over_the_limit_is_rejected` |
| T-RP-28-004 | fixture：001 で `L_asr` の行が無い | 列の欠け | `Err` | `Err` で `L_asr` を名指し（specified、RP-28 の「`L`、`L_asr`、`L_tr`」） | 陰性 | `cpu_run_without_l_asr_is_rejected` |
| T-RP-28-005 | fixture：CPU の run の値が GPU の run の行（RP-3）に混ざり、別の行が無い | 別の行ではない | `Err` | `Err`。GPU の合否の行に CPU の値を入れない（specified、RP-28 の「別の行に」） | 境界、陰性 | `cpu_values_mixed_into_the_gpu_row_are_rejected` |
| T-RP-28-006 | fixture：`gpu.fallback.cpu` が 0 件の GPU の run を CPU の run として渡した | 在るのに落ちていない | `Err` | `Err`。CPU の run として受け付けない（specified、RP-28 の陰性欄） | 陰性 | `gpu_run_passed_as_cpu_run_is_rejected` |
| T-RP-28-007 | fixture：GPU の run の同じ列 | 同じ test が GPU の run からも読める | `Ok` | 同じ関数が GPU の run から同じ列を出せる（specified、RP-28 の陽性対照） | 陽性 | `same_columns_are_read_from_a_gpu_run` |
| T-RP-28-008 | 実走：`CUDA_VISIBLE_DEVICES` を空にして `<ev> stream --wav lat-ja.wav --repeat 5` | 実走 | 実走 | 001 の形（specified、RP-28）。R8e が実測で上限を置き直す | 陽性 | `cpu_fallback_run_records_l_apart` |

### 4.39 RP-29（GPU 以外の資源が無くても止まらない）

test：`native/hayamimi/tests/rp29.rs`。受入 test の名前は `rp29_missing_resources_fall_back_and_say_so`。各場面の実走の行は、3.15 の表の持ち主の slice が足す。
`check_resource_fallback(scene, run)` は、場面ごとに資源を外した run と在る run を対で読む。場面と落ちた event の名前は 3.15 の表による（model の file、推論の library、llama-server、外の送り先、port、取り込みの装置、設定の file）。
外した run は、終了 code が 0、`final`（推論の library を外した場面では server の `ready`）が出て、その場面の `<domain>.fallback.<落ち先>` の log の event と `/events` の `fallback` が 1 件ずつ出る。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-29-001 | fixture：model の file を外した run。終了 code 0、`final` 1 件以上、`model.fallback.smaller` 1 件（`reason` = `missing`）、`/events` の `fallback` 1 件 | 外した run（落ちた形） | `Ok` | `Ok`（specified、RP-29、3.15 の model の行） | 陰性 | `missing_model_falls_back_and_says_so` |
| T-RP-29-002 | fixture：model の file が在る run。同じ場面の fallback の event が log と `/events` で 0 件 | 在るのに落ちない（001 の対） | `Ok` | `Ok`（specified、RP-29 の陽性対照） | 陽性 | `present_model_emits_no_fallback_event` |
| T-RP-29-003 | fixture：推論の library を外した run。終了 code 0、server の `ready` が出て `final` は 0 件、`asr.fallback.disabled` 1 件（`reason` = `library_unavailable`）、`/events` の `fallback` 1 件 | `final` を求めない場面 | `Ok` | `Ok`（specified、RP-29 の「推論の library を外した行では server の `ready`」） | 境界、陰性 | `missing_library_keeps_the_server_and_says_so` |
| T-RP-29-004 | fixture：001 と同じ外した run で、`model.fallback.smaller` が 0 件 | 落ちたのに言わない | `Err` | `Err` でその場面を名指し（specified、RP-29 の陰性欄） | 陰性 | `silent_fallback_is_rejected` |
| T-RP-29-005 | fixture：002 と同じ在る run で、`model.fallback.smaller` が 1 件 | 在るのに落ちる build | `Err` | `Err` でその場面を名指し（specified、RP-29 の陰性欄） | 陰性 | `fallback_with_the_resource_present_is_rejected` |
| T-RP-29-006 | fixture：001 と同じで、終了 code が 1 | 止まった build | `Err` | `Err`。終了 code は 0（specified、RP-29） | 境界、陰性 | `nonzero_exit_code_in_a_missing_resource_run_is_rejected` |
| T-RP-29-007 | fixture：001 と同じで、log の event は 1 件、`/events` の `fallback` が 0 件 | 画面へ送らない | `Err` | `Err` で `/events` を名指し（specified、3.15 の「段ごとに出す」） | 陰性 | `fallback_not_sent_to_events_is_rejected` |
| T-RP-29-008 | fixture：001 と同じで、`model.fallback.smaller` が 2 件 | 件数の上側 | `Err` | `Err`。1 件ずつ（specified、RP-29） | 境界、陰性 | `two_fallback_events_for_one_scene_are_rejected` |
| T-RP-29-009 | fixture：llama-server に届かない run。終了 code 0、原文の `final` が出る、`translate.fallback.held` 1 件（`reason` = `unreachable`） | 訳の保留 | `Ok` | `Ok`（specified、RP-29、3.15 の llama-server の行） | 陰性 | `unreachable_llama_server_holds_translation_and_says_so` |
| T-RP-29-010 | fixture：外の送り先に届かない run。`sink.fallback.local` 1 件、SRT に確定の訳が残る | 送り先の失敗 | `Ok` | `Ok`（specified、RP-29、3.15 の送り先の行） | 陰性 | `unreachable_sink_keeps_translations_locally` |
| T-RP-29-011 | fixture：port 8833 を塞いだ run で、実際の port が 8843、`server.fallback.port` 1 件（`reason` = `in_use`）、出した URL が実際の port | 次の番号から 10 個の内側（最後） | `Ok` | `Ok`。8834 から 8843 の 10 個（derived、3.15 の「次の番号から 10 個まで」を数えた値） | 境界、陰性 | `blocked_port_uses_the_tenth_next_port` |
| T-RP-29-012 | fixture：011 で実際の port が 8844 | 10 個の外側 | `Err` | `Err`（derived、8844 は 11 個目） | 境界、陰性 | `blocked_port_beyond_ten_next_ports_is_rejected` |
| T-RP-29-013 | fixture：出力の装置が無く、マイクだけが在る run。`capture.fallback.single` 1 件（`reason` = `device_missing`）。もう 1 つは両方無い run で `capture.fallback.none` 1 件 | 取り込みの装置 | `Ok` | `Ok`（specified、3.15 の取り込みの行。RP-10 の 006 と同じ型） | 陰性 | `missing_capture_devices_fall_back_to_single_or_none` |
| T-RP-29-014 | fixture：設定の file が壊れ、前の内容も無い run。`config.fallback.default` 1 件、効いている設定の値が `hayamimi config dump` の値と全部同じ | 既定の設定 | `Ok` | `Ok`（specified、3.15 の設定の行と RP-19 の dump） | 陰性 | `broken_config_without_previous_uses_the_dump_defaults` |
| T-RP-29-015 | fixture：014 で効いている設定の値が 1 つ dump と違う | 既定の値の食い違い | `Err` | `Err` でその key を名指し（specified、3.15） | 境界、陰性 | `default_config_differing_from_the_dump_is_rejected` |
| T-RP-29-016 | fixture：送り先が戻った run。`sink.fallback.cleared` 1 件と `/events` の `{"type": "fallback", "to": null}` 1 件 | 資源が戻った | `Ok` | `Ok`（specified、3.15 の「戻ったとき」） | 陽性 | `recovered_sink_emits_cleared_events` |
| T-RP-29-017 | fixture：016 で `sink.fallback.cleared` が 0 件 | 戻ったのに言わない | `Err` | `Err`（specified、3.15） | 陰性 | `recovery_without_cleared_event_is_rejected` |
| T-RP-29-018 | fixture：GPU を隠して起こし、実行中に device が戻った run。`gpu.fallback.cleared` が 0 件、段の provider は `cpu` のまま | GPU の段は戻さない | `Ok` | `Ok`（specified、3.15 の「GPU の段は起動時に置き場を決めるので、実行中には戻さない」） | 境界 | `gpu_stage_stays_on_cpu_when_the_device_returns` |
| T-RP-29-019 | fixture：fallback が 2 つ効いている間に client が接続し直した列。最初の 2 件が効いている fallback の 2 つ | 接続し直し | `Ok` | `Ok`（specified、3.15 の「最初にまとめて送る」） | 陽性 | `reconnected_client_receives_active_fallbacks_first` |
| T-RP-29-020 | fixture：019 で効いている fallback の 1 つが最初に来ない | まとめの欠け | `Err` | `Err` でその stage を名指し（specified、3.15） | 陰性 | `reconnected_client_missing_an_active_fallback_is_named` |
| T-RP-29-021 | 実走：場面ごとに資源を外した run と在る run の対（持ち主の slice が場面ごとに足す） | 実走 | 実走 | 001 または 003 と 002 の形（specified、RP-29） | 陰性、陽性 | `missing_resources_fall_back_in_real_runs` |
| T-RP-29-022 | fixture：認識器の tier の model が無い run。終了 code 0、`final` 1 件以上、`model.fallback.substitute` 1 件（`reason` = `missing`）、`/events` の `fallback`（`stage` = `recognizer`、`to` = `substitute`）1 件 | R3 の場面：認識器の代わり | `Ok` | `Ok`（specified、RP-29、3.15 の model の行の `model.fallback.substitute`） | 陰性 | `missing_recognizer_tier_is_substituted_and_says_so` |
| T-RP-29-023 | fixture：言語判定の model が無い run。`model.fallback.disabled` 1 件（`stage` = `lid`、`reason` = `missing`）、`/events` の `fallback`（`to` = `disabled`）1 件 | R3 の場面：言語判定を外す | `Ok` | `Ok`（specified、RP-29、3.15 の「言語判定は `--lang` の言語に固定」） | 陰性 | `missing_identifier_model_leaves_the_stage_out_and_says_so` |
| T-RP-29-024 | fixture：parakeet-ja の model が無い run。`model.fallback.disabled` 1 件（`stage` = `second_opinion`、`reason` = `missing`）、`/events` の `fallback` 1 件 | R3 の場面：second opinion を切る | `Ok` | `Ok`（specified、3.15 の「parakeet-ja が無ければ second opinion を切る」） | 陰性 | `missing_second_opinion_model_turns_it_off_and_says_so` |
| T-RP-29-025 | fixture：model が全部在る run。言語判定の場面と認識器の代わりの場面の event が log と `/events` で 0 件 | 在るのに落ちない（022、023 の対） | `Ok` | `Ok`（specified、RP-29 の陽性対照） | 陽性 | `present_identifier_emits_no_fallback_event` |
| T-RP-29-026 | fixture：023 と同じ外した run で、`model.fallback.disabled` が 0 件 | 外したのに言わない build（言語判定） | `Err` | `Err` で `lid` の場面を名指し（specified、RP-29 の陰性欄） | 陰性 | `silent_identifier_fallback_is_rejected` |
| T-RP-29-027 | 実走（`native/hayamimi/tests/rp29/real.rs` の `speaker_scene`）：話者の model（`campplus_sv.onnx`）を外した directory と、全部在る directory で 1 回ずつ | R4 の場面：話者を外す | 実走 | 外した run は終了 code 0、`final` が 1 件以上、`model.fallback.disabled`（`stage` = `speaker`、`reason` = `missing`）が 1 件、`/events` の `fallback`（`to` = `disabled`）が 1 件、印の付いた `final` と `speaker` の event が 0 件。在る run は同じ場面の fallback の event が 0 件で、`speaker` の event が 1 件以上（specified、RP-29、3.15 の「話者は印なし」） | 陰性、陽性 | `speaker_scene` |
| T-RP-29-028 | fixture：027 の外した run で `model.fallback.disabled` が 0 件の log | 外したのに言わない build（話者） | `Err` | `Err` で `speaker` の場面を名指し（specified、RP-29 の陰性欄） | 陰性 | `silent_speaker_fallback_is_rejected` |

### 4.40 RP-30（`fallback` の `stage` と `reason` の値の一覧が code と同じ）

test：`native/hayamimi/tests/rp30.rs`。受入 test の名前は `rp30_fallback_stage_and_reason_lists_match_the_code`。`#[ignore]` を持たない。
`check_fallback_lists(design, code)` は、`docs/design/rust_port.ja.md` の 3.15 の「`fallback` の event の `stage` の値は」の段落の 2 つの一覧と、`native/hayamimi/src/notice.rs` の `STAGES` と `REASONS` を集合として比べ、どちらかにだけ在る値を名指しして `Err` を返す。
fixture の docs は、実物の段落を file に写して 1 語だけ直した物を `native/hayamimi/tests/rp30/` に置く（反例を command や test の本文の文字列で運ばない）。
実物の読みは、`docs/design/rust_port.ja.md` と `native/hayamimi/src/notice.rs` をそのまま渡す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-30-001 | 実物の `rust_port.ja.md` と実物の `notice.rs` | 一致 | `Ok` | `Ok`（specified、RP-30 の「集合として code と一致する」） | 陽性 | `design_lists_equal_the_code_constants` |
| T-RP-30-002 | 実物の段落から読んだ値の数 | 読めていること | `stage` が 13 個、`reason` が 11 個 | 13 と 11（specified、RP-30 の陽性対照。001 の `Ok` が空と空の一致ではないことを示す） | 陽性 | `the_real_paragraph_yields_13_stages_and_11_reasons` |
| T-RP-30-003 | 実物の段落。括弧の中の `unreachable`、`unsupported_target` と、中継の `sink` | `hayamimi` の一覧の外の値 | `reason` の集合にこの 2 つが無い。`stage` の集合に `sink` が無い | 無い（specified、3.15 の「`hayamimi` が 11 個」と括弧書き。段落の読み方が括弧の中まで拾うと 002 の 11 が 13 になって落ちる） | 境界、陰性 | `values_named_for_the_relay_are_not_read_as_hayamimi_values` |
| T-RP-30-004 | fixture：`stage` の一覧から `lid` を消した docs | 一覧から 1 つ消す（stage） | `Err` | `Err` で `lid` を名指し（code にだけ在る。specified、RP-30 の陰性欄） | 陰性 | `a_stage_removed_from_the_design_is_named` |
| T-RP-30-005 | fixture：`reason` の一覧から `bad_value` を消した docs | 一覧から 1 つ消す（reason） | `Err` | `Err` で `bad_value` を名指し（specified、RP-30） | 陰性 | `a_reason_removed_from_the_design_is_named` |
| T-RP-30-006 | fixture：`stage` の一覧に code に無い `diarize` を足した docs | code に無い値を足す（stage） | `Err` | `Err` で `diarize` を名指し（docs にだけ在る。specified、RP-30） | 陰性 | `a_stage_added_to_the_design_only_is_named` |
| T-RP-30-007 | fixture：`reason` の一覧に code に無い `timeout` を足した docs | code に無い値を足す（reason） | `Err` | `Err` で `timeout` を名指し（specified、RP-30） | 陰性 | `a_reason_added_to_the_design_only_is_named` |
| T-RP-30-008 | fixture：実物の docs と、`STAGES` に docs に無い `extra_stage` を足した `notice.rs` の写し | code にだけ在る値（逆向き） | `Err` | `Err` で `extra_stage` を名指し（specified、RP-30 の「どちらかにだけ在る値」） | 陰性 | `a_stage_added_to_the_code_only_is_named` |
| T-RP-30-009 | fixture：2 つの一覧を並べ替えた docs（`stage` を逆順、`reason` を逆順） | 順 | `Ok` | `Ok`（specified、RP-30 の「順を問わず」） | 境界 | `list_order_does_not_matter` |
| T-RP-30-010 | fixture：「`fallback` の event の `stage` の値は」の段落を消した docs | 段落が無い（空と空の一致を通さない） | `Err` | `Err` で段落が無いことを名指し（implicit。値が 0 個と 0 個を一致として通すと 001 の `Ok` が何も測らない） | 陰性、境界 | `a_design_without_the_paragraph_is_rejected` |

### 4.41 RP-31（`L` の数え方）

test：`native/hayamimi-eval/tests/rp31.rs`。受入 test の名前は `rp31_latency_counts_only_shown_translations_of_their_own_utterance`。`#[ignore]` を持たない。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp31`。
`stream` の集計の関数（今の `build_samples` と `summarize`）へ、発話の窓の列と event の列を渡す。
R8h は、この関数が event の `id`、`status`、`text`、`t1_s`、`src_sha256` を読む形に直す。
関数の名前と引数の形は実装の席が決め、test はその綴りを 1 か所の定数に持つ（4.1）。

fixture は `native/hayamimi-eval/tests/rp31/` に file で置く。
出所は 2 つである。

- **R8e の Rust の run**：commit `c90242e` の `docs/results/r8e/rp3_rust.events.jsonl`（1729 行）と `rp3_rust.jsonl`（115 行。1 行が 1 標本で、`repeat`、`utt`、`t_start_us`、`t_end_us` が窓）。`--min-silence 0.30`。
- **R0 の Python の run**：`testdata/baseline/lat_ja_0.35_1` と `lat_ja_0.35_2` の `.events.jsonl` と `.jsonl`（git 管理外）。

test は git 管理外の file を読まない。
Python の slice も、下の表の範囲を一度だけ切り出した file を `native/hayamimi-eval/tests/rp31/` に commit し、test はその file だけを読む。
fixture の file が無ければ、skip せずに、その path を名指しした `Err` で落ちる（どの機械でも、Windows でも同じ）。

切り出す slice は 6 つで、窓は `.jsonl` の該当行、event は表の範囲の行である。

| slice | 出所 | 窓（`t_start_us`、`t_end_us`） | event の範囲（行の数） |
|---|---|---|---|
| A | R8e | repeat 1 の発話 1（54969、7634967）、2（11154964、17314945）、3（21354951、26734974） | `t_us` ≤ 29000000（44） |
| B | R8e | repeat 2 の発話 12（301272618、306472593）、13（308225226、316596860）、14（318116850、326574992） | 301500000 ≤ `t_us` ≤ 330000000（68） |
| PL | Python `_2` | repeat 5 の発話 20〜23 | 952265000 ≤ `t_us` ≤ 977300000（40） |
| PO | Python `_1` | repeat 5 の発話 20〜23 | 同じ範囲（40） |
| PB | Python `_2` | repeat 1 の発話 10、11 | 91886063 ≤ `t_us` ≤ 107500000（19） |
| PC | Python `_2` | repeat 1 の発話 13、14 | 112873709 ≤ `t_us` ≤ 134500000（37） |

```
git show c90242e:docs/results/r8e/rp3_rust.events.jsonl | jq -c 'select(.t_us <= 29000000)'
git show c90242e:docs/results/r8e/rp3_rust.jsonl | jq -c 'select(.repeat == 1 and .utt <= 3)'
git show c90242e:docs/results/r8e/rp3_rust.events.jsonl | jq -c 'select(.t_us >= 301500000 and .t_us <= 330000000)'
git show c90242e:docs/results/r8e/rp3_rust.jsonl | jq -c 'select(.repeat == 2 and .utt >= 12 and .utt <= 14)'
```

「足した」と書いた行は、実物の event を写して case 行が言う key だけを変えた物である。
写す元は A の `mic-1` の `held` の訳（`id` が `mic-1`、`of` が `final`、`src_sha256` が `86c6dc546f34858831daa226260cf714b1c3584eb0dec712aff6388cdca81e49`、`text` が空、時刻 9802292）である。
`mic-2` の確定文の sha256 は `68972f77abf17f7011c960d9dfbe44519d08add6bf055a19c75123964b7edeb1` である。

期待値の oracle は、別に書いた jq の式（下）と、`printf '%s' <確定文> | sha256sum` である。
集計の code とは別の式で、`final` を `t1_s` を µs の整数に直した値で窓に割り（窓は発話の始まりから次の発話の始まりの手前まで）、窓の最後の `final` の `id` が同じ `of` が `final` で `status` が `ok` で `text` が空でない最初の訳を `t_tr` にする。
R8e の全記録に掛けた式の `t_tr` 67 件は、`src_sha256` が窓の最後の `final` の確定文の sha256 と全件一致した。

```
jq -n --slurpfile S <windows.jsonl> --slurpfile E <events.jsonl> '
 ($S) as $s
 | ($E | map(select(.event.type == "final"))) as $finals
 | ($E | map(select(.event.type == "translation" and .event.of == "final"))) as $trs
 | [ $s[] | . as $w
   | ($s | map(select(.t_start_us > $w.t_start_us)) | (min_by(.t_start_us) // null)) as $next
   | [ $finals[] | select((.event.t1_s * 1000000) >= $w.t_start_us
       and ($next == null or (.event.t1_s * 1000000) < $next.t_start_us)) ] as $own
   | ($own | max_by(.event.t1_s)) as $closing
   | (if $closing == null or $closing.t_us < $w.t_end_us then null else $closing end) as $close
   | (if $close == null then null
      else ($trs | map(select(.event.id == $close.event.id and .event.status == "ok"
             and (.event.text | length) > 0)) | (min_by(.t_us) // null)) end) as $tr
   | {repeat: $w.repeat, utt: $w.utt, finals: ($own | length),
      t_final: $close.t_us, t_tr: $tr.t_us, t_end: $w.t_end_us} ]'
```

Python の run の「遅れて着いた疑い」は、`.jsonl` の行の並びに `jq -s '[ . as $s | range(0; length - 1) | select($s[.].finals == 0 and $s[.+1].finals >= 2) ] | length'` を掛けた値である（Python の `final` は `t1_s` を持たず、`.jsonl` は着いた時刻で割った現行の集計の出力）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-31-001 | A の発話 1（`mic-1` の `final`、`t1_s` が 7.7）。訳は `held` で `text` が空の 1 件だけ | `held` の空の訳だけ | `t_tr` 無し | 発話 1 は `finals` 1、`t_final` 8937248、`L_asr` 1302281 µs。`t_tr` と `L` と `L_tr` は無く、訳の取れない標本に数える。記録の旧い行は `t_tr_us` 9802292 で `L` が 2167325 µs（specified、3.1 の「`held` の訳と `text` が空の訳は `t_tr` に入れない」。時刻は jq の式） | 陰性 | `a_held_empty_translation_is_not_t_tr` |
| T-RP-31-002 | 001 に、時刻 10100000 の `ok` の訳（`id` が `mic-1`、`text` が非空、`src_sha256` が `mic-1` の物）を足す | `held` の後の `ok` | 10100000 | `t_tr` 10100000、`L` 2465033 µs、`L_tr` 1162752 µs。`held` の 9802292 は選ばれない（specified、3.1 と RP-31 の「同じ `id` の後の `ok` の訳が `t_tr` になる」） | 陽性 | `an_ok_translation_after_a_held_one_is_t_tr` |
| T-RP-31-003 | 001 の `held` の前、時刻 9700000 に 002 と同じ `ok` の訳を足す | `ok` が先、`held` が後 | 9700000 | `t_tr` 9700000、`L` 2065033 µs、`L_tr` 762752 µs。後ろの `held` は `t_tr` を動かさない（specified、3.1 の「初めて受けた」） | 境界 | `a_later_held_translation_does_not_replace_the_first_ok` |
| T-RP-31-004 | 001 の `held` の訳の `text` を非空にする | `status` だけで除く（`text` は非空） | `t_tr` 無し | `t_tr` 無し（specified、3.1。`status` と `text` は別の条件） | 境界、陰性 | `a_held_translation_with_text_is_not_t_tr` |
| T-RP-31-005 | 001 の `held` の訳を `status` が `ok` で `text` が空の物にする | `text` だけで除く | `t_tr` 無し | `t_tr` 無し（specified、3.1） | 境界、陰性 | `an_ok_translation_with_empty_text_is_not_t_tr` |
| T-RP-31-006 | 001 の `held` の訳を `status` が `unavailable` で `text` が空の物にする | `unavailable`（実物の記録に 0 件。足した行） | `t_tr` 無し | `t_tr` 無し（specified、3.1 の「`held` か `unavailable`」） | 陰性 | `an_unavailable_translation_is_not_t_tr` |
| T-RP-31-007 | 006 の `text` を非空にする | `unavailable` で `text` が非空 | `t_tr` 無し | `t_tr` 無し（specified、3.1） | 境界、陰性 | `an_unavailable_translation_with_text_is_not_t_tr` |
| T-RP-31-008 | 002 の訳の `src_sha256` を `mic-2` の確定文の sha256 にする | 原文が `final` と一致しない | `t_tr` 無し | `t_tr` 無し（specified、3.1 の「訳の元にした原文の文字列が `final` の文字列と一致」。RP-3 の 002 と同じ条件を Rust の run の `id` の結びの上で確かめる） | 陰性 | `a_translation_of_another_source_text_is_not_t_tr` |
| T-RP-31-009 | 002 の訳の `id` を `mic-2` にする（`src_sha256` は `mic-1` の物のまま） | `id` が違う | 発話 1 は `t_tr` 無し | 発話 1 は `t_tr` 無し。発話 2 の値（012）は動かない（specified、3.1 の「`final` と同じ `id` で結ぶ」） | 陰性 | `a_translation_with_another_id_is_not_tied_to_the_utterance` |
| T-RP-31-010 | 002 の訳の `of` を `partial` にする | 速報の訳 | `t_tr` 無し | `t_tr` 無し。速報の訳は `L` に入れない（specified、3.1 の「速報の訳の時刻は `L` に入れず」） | 陰性 | `a_partial_translation_is_not_t_tr` |
| T-RP-31-011 | A の発話 3（`mic-4`）と B の発話 12（`mic-37`）。`held` が無い | `held` の無い fixture | 旧い行と同じ | 発話 3 は `t_tr` 28267506、`L` 1532532 µs。発話 12 は `t_tr` 308008448、`L` 1535855 µs。どちらも記録の旧い行の `t_tr_us` と同じ（derived、`rp3_rust.jsonl` の行。jq の式も同じ値） | 陽性 | `utterances_without_a_held_translation_keep_their_l` |
| T-RP-31-012 | A の発話 2。訳は `mic-2` の `final` の分が 17051980 と 17267336、`mic-3` の分が 18483305（`stage` が `t`）と 18602637（`stage` が `e`）で、どれも `ok` | 最初の `ok` と、早い `final` の訳 | 18483305 | `t_tr` 18483305、`L` 1168360 µs、`L_tr` 92138 µs。後の 18602637 でも、`mic-2` の 17051980 でもない（specified、3.1。`mic-2` の訳は `src_sha256` が `mic-3` の確定文と違う） | 境界、陽性 | `t_tr_is_the_first_ok_translation_of_the_closing_final` |
| T-RP-31-013 | 012 と同じ。発話 2 の `final` は `mic-2`（`t1_s` 16.372、着いた時刻 16842250）と `mic-3`（`t1_s` 17.908、着いた時刻 18391167） | 1 つの発話に 2 つの `final` | `finals` 2 | `finals` 2、窓の最後の `final` は `mic-3`、`t_final` 18391167、`L_asr` 1076222 µs。`mic-2` は `t_end` 17314945 より前に着いた早い `final` で、閉じない（derived、現行の `build_samples` の「最後の `final` が閉じる」を `t1_s` の順に移した式） | 陽性 | `a_split_utterance_is_closed_by_its_last_final_by_t1_s` |
| T-RP-31-014 | B の発話 13。`final` は `mic-38`（`t1_s` 316.884、着いた時刻 318628342）。着いた時刻は発話 14 の始まり 318116850 より後 | 遅れて着いた `final` | `finals` 1 | 発話 13 は `finals` 1、`t_final` 318628342、`L_asr` 2031482 µs、`t_tr` 319522568（`mic-38` の `stage` が `e` の `ok` の訳）、`L` 2925708 µs、`L_tr` 894226 µs。記録の旧い行は `finals` 0 で「final なし」（specified、3.1 と RP-31。値は jq の式） | 陰性、陽性 | `a_late_final_belongs_to_its_own_utterance_by_t1_s` |
| T-RP-31-015 | B の発話 14。`final` は `mic-39`（`t1_s` 320.66）、`mic-40`（322.964）、`mic-41`（326.964） | 遅れて着いた `final` を取り除かれた次の窓 | `finals` 3 | 発話 14 は `finals` 3（記録の旧い行は 4）、`t_final` 327445514、`t_tr` 327643922（`mic-41` の `stage` が `t`）、`L` 1068930 µs、`L_asr` 870522 µs（derived、jq の式） | 陽性 | `the_next_utterance_loses_the_late_final_it_never_owned` |
| T-RP-31-016 | B の発話 14 の窓の `t_start_us` を 318117000 に変え、`mic-38` の `t1_s` を 318.116 にする（足した行） | 窓の境界の手前 | 発話 13 | `mic-38` は発話 13 の標本に入る。`t1_s` を µs の整数に直してから比べる（derived。設計は窓の境界を書いていない。窓は始まりを含み次の始まりを含まない） | 境界 | `t1_s_just_before_the_next_start_belongs_to_the_earlier_utterance` |
| T-RP-31-017 | 016 の `t1_s` を 318.117 にする | 窓の境界の上 | 発話 14 | `mic-38` は発話 14 の標本に入り、発話 13 は `finals` 0（derived。001 から 016 と同じ規則） | 境界 | `t1_s_equal_to_the_next_start_belongs_to_the_next_utterance` |
| T-RP-31-018 | R8e の全記録（`docs/results/r8e/` の 2 file をそのまま読む。file が無ければその path を名指しして `Err`） | 全記録 | 表の値 | 標本 115、`with_final` 101、`with_translation` 67、`untranslated` 48、`finals_total` 125、`l_tr_negative` 0。`L` は n 67、p50 1533 ms、p95 2752 ms。`L_asr` は n 101、p50 1302 ms、p95 2050 ms。`L_tr` は n 67、p50 316 ms、p95 1237 ms。記録の旧い値は 93、93、22、125 と、`L` 1710 ms、3145 ms（pseudo、jq の式。有効の判定の前の値で、`summarize` が出す欄と同じ母数） | 陽性、陰性 | `the_whole_r8e_record_gives_the_corrected_counts` |
| T-RP-31-019 | 018 と同じ記録 | 新旧の差 | 8 標本 | 旧い行に `t_final` が無く新しい数え方で有る標本は 8 で、(repeat、utt) が (2、13)、(3、11)、(3、22)、(4、12)、(4、13)、(4、22)、(5、12)、(5、13)。このうち訳が取れるのは (2、13)、(4、12)、(4、13)、(5、12)、(5、13) の 5 つで、(3、11)、(3、22)、(4、22) は `held` で訳が無い。旧い行と新しい数え方で `t_final` の値が変わる標本は 0（pseudo、jq の式。設計の「遅れて着いた `final` の 8 標本」と一致） | 陽性 | `eight_samples_regain_their_late_final_and_none_changes_its_time` |
| T-RP-31-020 | PL（Python `_2` の repeat 5 の発話 20〜23）。`finals` は順に 1、0、2、1 | 前の窓が「final なし」で次の窓に `final` が 2 件（境界の等号） | 遅れた疑い 1 | 別の列の「遅れて着いた疑い」が 1。発話 21 が `finals` 0、発話 22 が `finals` 2（specified、3.1 と RP-31。jq の式が同じ値） | 陽性、境界 | `a_python_late_final_is_counted_in_its_own_column` |
| T-RP-31-021 | PO（Python `_1` の同じ 4 窓）。`finals` は順に 1、1、1、1 | 同じ発話で、`final` が窓の中に着いた run | 遅れた疑い 0 | 「遅れて着いた疑い」が 0。020 と同じ発話の対で、`_2` の run だけ発話 21 の `final` が発話 22 の始まり 964873298 より後に着く（specified、RP-31。jq の式が同じ値） | 陰性 | `a_python_run_with_on_time_finals_counts_no_suspect` |
| T-RP-31-022 | PB（Python `_2` の repeat 1 の発話 10、11）。`finals` は 0、1 | 次の窓の `final` が 1 件（境界の下） | 遅れた疑い 0 | 0。次の窓の `final` が 2 件に足りない（specified、3.1 の「2 件以上」） | 境界、陰性 | `one_final_in_the_next_window_is_not_a_suspect` |
| T-RP-31-023 | PC（Python `_2` の repeat 1 の発話 13、14）。`finals` は 1、3 | 前の窓に `final` が在る | 遅れた疑い 0 | 0。次の窓に `final` が 3 件在っても、前の窓が「final なし」でなければ数えない（specified、3.1） | 境界、陰性 | `a_previous_window_with_a_final_is_not_a_suspect` |
| T-RP-31-024 | PL の発話 20、22、23 の標本 | Python の訳は `status` の key が無い | 旧い行と同じ | 発話 20 は `t_tr` 957098241、`L` 1187137 µs。発話 22 は `t_tr` 969859404、`L` 1247548 µs。発話 23 は `t_tr` 975384593、`L` 1132919 µs。`status` の key が無い訳を、`text` が非空なら `ok` と同じに数える。訳は着いた順に、窓の最後の `final` の後で次の `final` の前の最初の物に結ぶ（derived、記録の旧い行。RB-5 の規則） | 陽性 | `a_python_translation_without_status_still_counts` |
| T-RP-31-025 | PL の発話 22 の `final` の後に、`text` が空の `translation` を 1 件足す（時刻は 969800000。本物の訳 969859404 より前） | Python の空の訳 | 969859404 | 発話 22 の `t_tr` は 969859404 のまま。空の訳は数えない（specified、3.1 の「`text` が空の訳は `t_tr` に入れない」は Python の run にも掛かる） | 境界、陰性 | `a_python_translation_with_empty_text_is_skipped` |
| T-RP-31-026 | B（Rust の run）の集計結果 | Rust の run の「遅れた疑い」の欄 | 数えない | 欄は「数えていない」を表す値（`None` など）で、`0` ではない。020 の `1`、021 の `0` と区別できる（specified、RP-31 の「Python の run では…別の列に出る」。数えていない 0 と数えて 0 を分ける） | 陰性 | `the_suspect_column_is_not_counted_for_a_rust_run` |
| T-RP-31-027 | 実走：Rust の run と Python の run を `<ev> stream` で 1 本ずつ | 実走 | 実走 | 両方の結果の file に、`with_translation` と訳の取れない標本の数（`untranslated`）と、Python の run だけ「遅れて着いた疑い」が別々の欄で在る（specified、3.1 の「3 つの数と、訳の取れない有効な標本の数は、結果の file に別々に書く」） | 陽性 | `real_runs_write_the_counts_in_separate_fields` |

001 から 019 は、実物の記録から切り出した値と足した行の対で、旧い数え方を落とす。
001 は空の訳の時刻 9802292 を `t_tr` にする数え方を、014 は着いた時刻で窓に割る数え方を落とす。
この 2 つの旧い数え方は、記録の `rp3_rust.jsonl` の行（発話 1 の `t_tr_us`、repeat 2 の発話 13 の `finals` 0）がそのまま持つ。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 窓の境界。3.1 は「`t1_s` で結ぶ」とだけ書く。016 と 017 は「始まりを含み次の始まりを含まない」を置いた。R8e の記録に掛けると、旧い行と食い違う標本が設計の 8 と一致するので、この規則で記録とは合う。
- 直す方向。3.16 は、`held` の空の訳の数え方と遅れて着いた `final` の欠けが、どちらも Rust の `L` を小さく見せると書く。018 の jq の式で数え直すと、R8e の記録では `L` は p50 が 1710 から 1533 ms、p95 が 3145 から 2752 ms へ小さくなる。`held` の 31 行は `L` が大きい行（その行だけの p50 2081 ms、p95 4125 ms）で、数えなくなると母数から抜ける。遅れて着いた `final` の 8 標本のうち訳が取れる 5 つ（`L` は 2294〜2925 ms）は、大きい側に足される。母数は 93 から 67 に減り、差し引きで `L` は小さくなる。V が止める行ほど `L` が大きいので、止める行が増えるほど、訳の取れた標本だけで数える `L` は小さく出る。RP-32 は訳の取れない標本の数を合否に使わないので、この向きは受入で捕まらない。
- Rust の run が `id` と `t1_s` の無い `final` を含むときの扱い。

### 4.42 RP-32（同じ負荷で交互に測った `L`）

test：`native/hayamimi-eval/tests/rp32.rs`。受入 test の名前は `rp32_alternated_runs_under_the_same_load_beat_python`。実走の行（T-RP-32-029）だけが `#[ignore]` を持ち、run の file と `/proc/loadavg` の記録を読む。ほかの行は fixture を読み、`--ignored` を付けずに通る。
有効の線を Rust の標本ごとに `asr.endpoint.decided` の `rule` で選ぶ形（D 3.1 の終端不正の例外、6 の RP-32 の行）の受入 test の名前は `rp32_validity_line_follows_the_rule_that_ended_each_final` で、置き場は同じ `native/hayamimi-eval/tests/rp32.rs` である。陰性の case は、`rule` が `silence` の標本に `EARLY_MIN_SILENCE_MS` を当てる判定と、`asr.endpoint.decided` に結べない `final` を既定の線で数える判定である。この名前の下の case 行は test-designer の席が書く（D 10 の H86）。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp32`。実走の行はこれに `-- --ignored` を足す。
`check_alternated_runs(runs)` は、流した順の run の列を受ける。1 つの run は、実装（Python か Rust）、前と後の `/proc/loadavg` の 1 行、RP-31 の数え方で出した標本の列である。
次の 4 つを確かめる。順と対の数と load の記録（001 から 014）、3 対を合わせた標本の合否（015 から 022）、列の出力（023 から 028）、dir の `pairs.txt` の読み方（030 から 036）。
関数の名前と引数の形は実装の席が決める。

**F32**（式で作る。実測のふりをしない）：実装 X が Python か Rust、対 k が 1〜3、発話 u が 1〜19 ごとに標本が 1 つ。
`t_end` は 0、`L_asr` は 700 + u ms、`L` は 1000 + 10u + k ms（したがって `L_tr` は 300 + 9u + k ms）。
Rust の標本は、書いた箇所のほかは Python と同じにする。
3 対の標本は 57 個で、小さい方から数えた p50 は 29 番目（(50 × 57 + 99) / 100）、p95 は 55 番目（(95 × 57 + 99) / 100）である。
`L` の 29 番目は u = 10、k = 2 の 1102 ms、55 番目は u = 19、k = 1 の 1191 ms。
`L_asr` の 29 番目は 710 ms、55 番目は 719 ms。`L_tr` の 29 番目は 392 ms、55 番目は 472 ms。
比較集合は 19 発話で、対の比較の閾値は RP-3 の T-RP-3-018 の式で (19 × 20 + 22) / 23 = 17。
load の 1 行は `/proc/loadavg` の書式（`0.50 0.50 0.50 1/300 1000`）で、先頭の欄が 1 分の load である。値は小数 2 桁なので、10 の -2 乗の整数に読んで比べる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-32-001 | F32 そのまま。順は Python、Rust、Python、Rust、Python、Rust。前の load は全 run で `0.50` | 正しい順、load、Rust が Python と同じ | `Ok` | `Ok`（specified、RP-32 の「以下」。等号側） | 境界、陽性 | `three_alternated_pairs_with_equal_values_pass` |
| T-RP-32-002 | 001 で順を Python、Python、Rust、Rust、Python、Rust にする | Python が続けて 2 回 | `Err` | `Err` で 2 番目の run を名指し（specified、RP-32 の陰性欄） | 陰性 | `two_python_runs_in_a_row_are_rejected` |
| T-RP-32-003 | 001 で順を Rust、Python、Rust、Python、Rust、Python にする | Rust が先頭 | `Err` | `Err` で 1 番目の run を名指し（specified、3.16 の 4 の「Python、Rust、Python、Rust … の順」） | 陰性 | `a_run_order_that_starts_with_rust_is_rejected` |
| T-RP-32-004 | 001 の最後の Rust の run を除いた 5 本 | 対が欠ける | `Err` | `Err`。最後の Python の run が対を持たない（specified、RP-32 の「3 対」） | 境界、陰性 | `five_runs_leave_the_last_pair_unfinished` |
| T-RP-32-005 | 001 の先頭の 2 対だけ（4 本）。結果に対の数 2 が記録されている | 2 対（記録あり） | `Ok` | `Ok`。3 対が収まらなかった場合の 2 対を、数を記録していれば受ける（specified、3.16 の順と置き場） | 境界 | `two_pairs_with_the_count_recorded_are_accepted` |
| T-RP-32-006 | 005 で対の数の記録が無い | 2 対（記録なし） | `Err` | `Err` で記録が無いことを名指し（specified、3.16 の「その数を記録する」） | 陰性 | `two_pairs_without_the_recorded_count_are_rejected` |
| T-RP-32-007 | 先頭の 1 対だけ（2 本）。対の数 1 が記録されている | 1 対 | `Err` | `Err`。2 対が下限（specified、3.16 の順と置き場。3 対が基本で、2 対まで） | 境界、陰性 | `one_pair_is_rejected_even_with_the_count_recorded` |
| T-RP-32-008 | 001 で 4 番目の run の前の load を `1.99 0.50 0.50 1/300 1000` にする | load の上側の境界の手前 | `Ok` | `Ok`（specified、RP-32 の「2 未満」。199 < 200） | 境界、陽性 | `a_load_of_1_99_before_a_run_passes` |
| T-RP-32-009 | 008 の load を `2.00 0.50 0.50 1/300 1000` にする | 境界の値 | `Err` | `Err` で 4 番目の run を名指し（specified、RP-32 の「2 未満」。200 は 2 未満でない） | 境界、陰性 | `a_load_of_2_00_before_a_run_is_rejected` |
| T-RP-32-010 | 008 の load を `0.00 0.00 0.00 1/300 1000` にする | load の下限 | `Ok` | `Ok`（specified、RP-32） | 境界 | `a_load_of_0_00_before_a_run_passes` |
| T-RP-32-011 | 001 で 4 番目の run の前の load の記録が無い | 前の記録の欠け | `Err` | `Err` で 4 番目の run を名指し。値は出さない（specified、RP-32 の陰性欄「load の記録が無い run」） | 陰性 | `a_run_without_a_load_before_record_is_rejected` |
| T-RP-32-012 | 001 で 4 番目の run の後の load の記録が無い | 後の記録の欠け | `Err` | `Err` で 4 番目の run を名指し（specified、3.16 の 4 の「各 run の前と後の `/proc/loadavg`」） | 陰性 | `a_run_without_a_load_after_record_is_rejected` |
| T-RP-32-013 | 001 で 4 番目の run の前の load の行が `n/a` | 読めない値 | `Err` | `Err`。読めない値を 0 と読んで 2 未満に通さない（implicit） | 境界、陰性 | `an_unreadable_load_line_is_rejected_not_read_as_zero` |
| T-RP-32-014 | 001 で 4 番目の run の前の load が `1.99`、後が `9.99` | 後の load | `Ok` | `Ok`。後の load は記録だけで、上限は前の load にだけ在る（specified、RP-32 の「各 run の前の 1 分の load」）。出力に後の load の値が残る | 境界 | `the_load_after_a_run_is_recorded_but_not_judged` |
| T-RP-32-015 | F32 で Rust の `L` の 29 番目（u = 10、k = 2）を +1 ms | p50 の順位に当たる標本 | `Err` | `Err` で p50 を名指し。Rust の 29 番目は 1103 ms で Python の 1102 ms を超える（specified、RP-32。nearest-rank の式 (p × n + 99) / 100） | 境界、陰性 | `p50_one_millisecond_over_at_the_29th_sample_is_rejected` |
| T-RP-32-016 | F32 で Rust の `L` の 30 番目（u = 10、k = 3）だけを +1 ms | p50 の順位の次 | `Ok` | `Ok`。29 番目は 1102 ms のまま（derived、nearest-rank。順位が p50 を決める） | 境界 | `p50_is_decided_by_the_29th_sample_only` |
| T-RP-32-017 | F32 で Rust の `L` の 55、56、57 番目（u = 19 の 3 つ）を +1 ms | p95 の順位に当たる標本 | `Err` | `Err` で p95 を名指し。Rust の 55 番目は 1192 ms で Python の 1191 ms を超える。p50 は通る（specified、RP-32） | 境界、陰性 | `p95_one_millisecond_over_at_the_55th_sample_is_rejected` |
| T-RP-32-018 | F32 で Rust の `L` の 56、57 番目（u = 19 の k = 2、3）だけを +1 ms | p95 の順位の次 | `Ok` | `Ok`。55 番目は 1191 ms のまま（derived、nearest-rank） | 境界 | `p95_is_decided_by_the_55th_sample_only` |
| T-RP-32-019 | F32 で Rust の u = 1、2 の標本 6 つを +1 ms（発話ごとの中央値が Python を超えるのは 2 発話） | 対の比較の閾値の上 | `Ok` | `Ok`。比較集合 19 発話のうち 17 発話で Rust の中央値が Python 以下。p50 と p95 の順位に当たる標本は動かない（specified、RP-32 の「RP-3 の対の比較（17 発話以上）」と RP-3 の 007） | 境界、陽性 | `seventeen_of_19_utterances_not_slower_pass` |
| T-RP-32-020 | 019 に加えて u = 3 の標本 3 つを +1 ms（超えるのは 3 発話） | 閾値の下 | `Err` | `Err` で対の比較を名指し（16 発話）。p50 と p95 は通る（specified、RP-3 の 008） | 境界、陰性 | `sixteen_of_19_utterances_not_slower_are_rejected_though_p50_and_p95_pass` |
| T-RP-32-021 | F32 で Python の 3 つの run がどれも u = 19 の標本を持たない（`final` なし）。Rust は u = 19 の標本を持つ | 比較集合は Python の run だけで決まる | 18 発話 | 関数が返す比較集合は 18 発話で u = 19 を含まず、閾値は (18 × 20 + 22) / 23 = 16。Rust の標本があっても入らない。判定全体は見ない（この入力では p95 が `Err` になる。Rust は 57 標本の 55 番目で 1191 ms、Python は 54 標本の 52 番目で 1181 ms）（specified、3.1 の「比較集合は Python の run だけで決め」、RP-3 の 020 と同じ式） | 陽性、陰性 | `the_comparison_set_ignores_utterances_python_never_finished` |
| T-RP-32-022 | F32 に、RP-31 の A の発話 1（`held` の空の訳だけ）の形の標本を Rust に 1 つ足す | `held` の標本 | `L` の標本は 57 のまま | Rust の `L` の標本の数は 57 で、`held` の標本は訳の取れない標本の数に入る。RP-31 の数え方を通した標本を受ける（specified、RP-32 の「RP-31 の数え方」） | 陰性 | `a_held_sample_is_not_counted_in_l` |
| T-RP-32-023 | F32 | 列の出力 | 6 つの値 | `L_asr` の p50 710 ms、p95 719 ms。`L_tr` の p50 392 ms、p95 472 ms。「訳」の段の p50、p95 の列が在る（392 ms、472 ms）。`L` の p50 1102 ms、p95 1191 ms（derived、F32 の式。「訳」の段は 3.1 の `L_tr` と同じ定義の値で、設計は 2 つを別の列として挙げる） | 陽性 | `l_asr_l_tr_and_translation_stage_columns_are_written` |
| T-RP-32-024 | 023 で「訳」の段の列が無い出力 | 列の欠け | `Err` | `Err` で「訳」の段を名指し（specified、RP-32 の「別の列に出す」） | 陰性 | `a_missing_translation_stage_column_is_rejected` |
| T-RP-32-025 | 023 で `L_tr` の列が無い出力 | 列の欠け | `Err` | `Err` で `L_tr` を名指し（specified、RP-32） | 陰性 | `a_missing_l_tr_column_is_rejected` |
| T-RP-32-026 | R0 の Python 同士の 2 run（`testdata/baseline/lat_ja_0.35_1.jsonl` と `_2.jsonl`）を、順を問わない 1 対として渡す | Python 同士の対 | 列が出る | `L` の標本の数が 95 と 94、列は 023 と同じ 6 つ以上。有効の判定は 350 ms（pseudo、jq。`select(.t_final_us != null)` と `((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) >= 350` と `.t_tr_us != null` の数を数えた値。RB-5 の分割と同じ式）。この 1 対は合否の判定には掛けない（RP-32 の陽性欄の「Python 同士の 2 run の対から同じ列を出せる」） | 陽性 | `python_against_python_pair_yields_the_same_columns` |
| T-RP-32-027 | 026 の 2 run に、`_1` を Rust として渡した 1 対 | 同じ run を 2 回 | 差 0 | `L` の p50 と p95 の差が 0 で、`Ok`（pseudo。同じ入力は差を持たない） | 陽性 | `the_same_run_against_itself_has_no_difference` |
| T-RP-32-028 | R8e の Rust の run（RP-31 の 018 の数え方）と R0 の Python の run（`lat_ja_0.30_1`）を 1 対として列を出す。対の数と load は掛けない。有効の判定は両方 `min_silence` 300 ms | 実物の記録 | `L` の欄の値 | Rust の `L` が p50 1533 ms、p95 2752 ms（n 66）。Python の `L` が p50 1481 ms、p95 2008 ms（n 95）。Rust の p50 と p95 のどちらも Python を超える（pseudo、jq。有効の判定は `((t_final − t_end) / 1000 − final の latency_ms) >= 300`）。数え方を直した後も残る差を、判定の道具が「超える」と読む確かめ | 陰性 | `the_recorded_r8e_gap_is_read_as_slower_than_python` |
| T-RP-32-029 | 実走：Python、Rust、Python、Rust、Python、Rust の 6 run と、各 run の前後の `/proc/loadavg` | 実走 | 実走 | 001 の形で `Ok`（specified、RP-32） | 陽性 | `rp32_alternated_runs_under_the_same_load_beat_python` |
| T-RP-32-030 | F32 の 3 対（6 run、順は 001 と同じ）を置いた dir。`pairs.txt` は無い | `pairs.txt` が無い 3 対 | `Ok` | `Ok`。無い dir は 3 対として読む（specified、D 3.16 の 4 の「順と置き場」。dir の file の名前は実装の席が決める） | 境界、陽性 | `a_dir_without_pairs_txt_is_read_as_three_pairs` |
| T-RP-32-031 | 030 から 3 対目（5、6 番目の run）を除いた 4 本の dir。`pairs.txt` は無い | `pairs.txt` が無い 2 対 | `Err` | `Err` で 3 対目が欠けていることを名指し。無い dir は 3 対として読むので、在る数に合わせて 2 対とは読まない（specified、D 3.16 の 4 の「順と置き場」）。030 の陰性の対照 | 境界、陰性 | `a_dir_without_pairs_txt_and_two_pairs_is_missing_the_third` |
| T-RP-32-032 | 031 の dir に、中身が `2` の 1 行の `pairs.txt` を足す | 2 と書いた 2 対 | `Ok` | `Ok`。在る dir はその数の対を読む（specified、D 3.16 の 4 の「順と置き場」）。005 と同じ 2 対を、dir の読み方から通す | 境界、陽性 | `pairs_txt_of_2_accepts_two_pairs` |
| T-RP-32-033 | 030 の dir（3 対）に、中身が `2` の 1 行の `pairs.txt` を足す | 2 と書いたのに 3 対ある | `Err` | `Err` で数が合わないことを名指し。記録は 2、在る対は 3 と、どちらの数も出す（specified、D 3.16 の 4 の「`pairs.txt` の数と在る対の数が違う時」）。032 の陰性の対照 | 境界、陰性 | `pairs_txt_of_2_with_three_pairs_present_is_rejected` |
| T-RP-32-034 | 031 の dir に、中身が次の 4 つのどれかの `pairs.txt` を足した 4 入力。`two`、空（0 byte）、`2 pairs`、`-2` | 読めない内容 | 4 入力とも `Err` | どの入力も `Err` で数が読めないことを名指し。数が合わないとは名指さない。`2 pairs` と `-2` を `2` と読んで 032 の `Ok` に通さない。落ちた入力は assert の message に出す（specified、D 3.16 の 4 の「数が読めない時」）。4 つは別の同値類（語、空、後ろに字、符号） | 陰性 | `an_unreadable_pairs_txt_is_rejected_as_unreadable` |
| T-RP-32-035 | 先頭の 1 対だけ（2 本）の dir。中身が `1` の 1 行の `pairs.txt` | 1（下限の下） | `Err` | `Err` で 1 対は下限に満たないことを名指し。記録の 1 と在る 1 対は合っているので、数が合わないとは名指さない（specified、D 3.16 の 4 の「`1` の時」）。032 の 2 が下限の上、007 は判定関数の側の同じ境界 | 境界、陰性 | `pairs_txt_of_1_is_rejected` |
| T-RP-32-036 | 030 の dir（3 対）に、中身が `3` の 1 行の `pairs.txt` を足す | 3 と書いた 3 対 | `Ok` | `Ok`。`pairs.txt` が無い 030 と同じ結果（specified、D 3.16 の 4 の「在る dir はその数の対を読む」）。033 の `2` を `3` にした陽性の対照 | 境界、陽性 | `pairs_txt_of_3_accepts_three_pairs` |
| T-RP-32-037 | F32 の **S50**：Rust の k = 3 の run の u = 13〜19 の 7 標本を、訳の取れない標本にする（RP-31 の A の発話 1 の形。`held` の空の訳だけで `t_tr` が無い。`final` は在り有効のまま）。ほかは F32 と同じ | 訳の取れた割合の下限の上。有効 57、訳の取れた 50。3 つの run に分けると k = 3 の run は 12 / 19 で 20 / 23 に足りない | 20 / 23 以上（整数で `訳の取れた × 23 ≥ 有効 × 20`） | `Ok`。50 × 23 = 1150 ≥ 57 × 20 = 1140。Rust の `L` は n 50 で p50 1091 ms、p95 1182 ms（nearest-rank の 25 番目と 48 番目）で、どちらも Python の 1102 ms、1191 ms 以下。比較集合 19 発話のすべてで Rust の中央値が Python 以下で、閾値 17 を超える。割合は run ごとではなく Rust の 3 run を合わせた標本で数える（derived、RP-32 の「訳の取れた割合が 20 / 23 以上」と D 3.18。値は awk で F32 の式から別に組んで照らした） | 境界、陽性 | `fifty_translated_of_57_valid_samples_pass_the_share` |
| T-RP-32-038 | 037 に加えて、Rust の k = 2 の run の u = 19 の 1 標本も訳の取れない標本にする（S49） | 下限の下。有効 57、訳の取れた 49 | 同上 | `Err`。訳の取れた標本 49、有効な標本 57、必要な割合 20 / 23 を名指す（49 × 23 = 1127 < 1140）。ほかの理由は載らない：Rust の `L` は n 49 で p50 1091 ms、p95 1181 ms、比較集合 19 発話のうち 19 発話で Rust が Python 以下で、3 つとも通る。`L` が Python 以下でも落ちる、が RP-32 の陰性欄「訳の取れた割合が 20 / 23 未満の Rust の run」の形（specified、RP-32 と D 3.18 の「RP-32 の判定の直し」。値は awk で照らした）。037 の陰性の対照で、`check_translated_share` を呼ばない今の判定ではこの入力が `Ok` になり落ちる | 境界、陰性 | `forty_nine_translated_of_57_valid_samples_fail_the_share_alone` |
| T-RP-32-039 | 038 に、Rust のすべての標本の `L` を +30 ms にした入力（全標本の `L` を同じだけ遅らせる） | 割合も `L` も対の比較も落ちる | `Err` | `Err`。訳の取れた割合（49 / 57）、`L` の p50（1121 ms が 1102 ms を超える）、p95（1211 ms が 1191 ms を超える）、対の比較（19 発話のうち Rust が Python 以下は 0 で閾値 17 に足りない）の 4 つがすべて載る。割合の失敗が `L` の失敗に隠れず、`L` の失敗が割合の失敗に隠れない（D 3.18 の「`judge_pooled` の失敗の列に足す」。derived、awk で照らした） | 陰性 | `a_short_share_is_listed_beside_the_other_failures` |
| T-RP-32-040 | 037 の入力に、Rust の k = 1 の run へ標本を 8 つ足す：`final` の無い標本 5 つ（repeat 2、u = 1〜5）と、`L_asr − 復号` が 299.999 ms の標本 3 つ（repeat 2、u = 6〜8。終端不正）。有効の判定は RP-32 の `min_silence` 300 ms | 分母に入らない標本。終端不正の境界は 300.000 ms が有効で 299.999 ms が無効 | 20 / 23 以上 | `Ok`。有効 57、訳の取れた 50 のまま（3.1 の「有効」に入らない標本は分母にも分子にも入らない）。足した 8 つを分母に入れると有効 65 で、50 × 23 = 1150 < 65 × 20 = 1300 となり落ちるので、`Ok` は足した標本を数えていないことを示す（specified、RP-32 の「分母は発話の標本のうち 3.1 の『有効』に入る物の数」） | 境界、陽性 | `samples_that_are_not_valid_stay_out_of_the_denominator` |
| T-RP-32-041 | 037 の入力で、Rust の全標本の `finals`（その標本に結ばれた `final` の数）を 2 にする | 分母は発話の標本の数で、`final` の数ではない | 20 / 23 以上 | `Ok`。有効 57、訳の取れた 50 のまま。`final` の数 114 を分母にすると 50 × 23 = 1150 < 114 × 20 = 2280 で落ちる（specified、RP-32 の「`final` の数や訳の依頼の数では数えない」） | 境界、陽性 | `the_denominator_counts_samples_not_finals` |
| T-RP-32-042 | F32 の **PY49**：Python の k = 1 の run の u = 1〜8 の 8 標本を、訳の取れない標本にする。Rust は F32 のまま（57 すべて訳が取れる） | Python 側の欠け。Python の割合は 49 / 57 で 20 / 23 に足りない | Rust の割合だけを合否に入れる | `Ok`。比較集合は Python の全 run で訳の取れた発話の 11（u = 9〜19）、閾値 (11 × 20 + 22) / 23 = 10、Rust が Python 以下の発話は 11。`L` は Python が p50 1113 ms、p95 1191 ms（n 49）で、Rust の 1102 ms、1191 ms（n 57）が以下。出力の割合の列に Rust の 57 / 57 と Python の 49 / 57 の両方が出る（specified、RP-32 の「Python の同じ割合からの欠けも別の列に出す」。値は awk で照らした）。Python の割合を合否に入れる判定は、この入力を落として落ちる | 陰性 | `a_short_python_share_is_recorded_and_not_judged` |
| T-RP-32-043 | F32 とは別の入力 **F32L**：Python と Rust の 3 対（6 run）が、23 発話 × 5 回 = 115 標本を持つ。全標本の `L` は 1000 ms、有効の作り方は F32 と同じ。Rust の 3 run を合わせて訳の取れない標本を 45 にする（発話 1〜22 が各 2 標本、発話 23 が 1 標本。どの発話にも訳の取れた標本が残る） | 設計の標本の大きさでの下限。有効 345、訳の取れた 300 | 20 / 23 以上 | `Ok`。300 × 23 = 6900 = 345 × 20 で等号。`L` の p50 と p95 は Rust も 1000 ms で Python 以下。比較集合 23 発話、閾値 (23 × 20 + 22) / 23 = 20 に対して 23 発話すべてで Rust が Python 以下（derived、345 × 20 / 23 = 300 を awk で割り切れることを確かめた） | 境界、陽性 | `three_hundred_of_345_valid_samples_pass_the_share_exactly` |
| T-RP-32-044 | 043 に、発話 23 の 2 つ目の標本も訳の取れない標本にする（訳の取れた 299） | 043 の 1 つ下 | 同上 | `Err`。訳の取れた標本 299、有効な標本 345 を名指す（299 × 23 = 6877 < 6900）。`L` と対の比較は通り、載る理由はこの 1 つだけ（specified、RP-32 の陰性欄）。043 と 044 は訳の取れた数だけが 1 違い、割合の比べを整数で行うことと、等号を通すことを両側から固定する | 境界、陰性 | `two_hundred_ninety_nine_of_345_valid_samples_fail_the_share` |
| T-RP-32-045 | `docs/results/rp32/` の 3 対（実物の記録）を `check_dir` に渡す。Rust の 3 run を合わせた有効な標本と、そのうち `t_tr_us` が null でない標本の数を `jq` で数える（`min_silence` 300 ms） | 実物の記録の割合。この dir は `fallback` の event を持たない | 有効 252、訳の取れた 249 | 判定の `Result` がどちらでも、載る理由に訳の取れた割合は無い。出力の割合の列は Rust の 249 / 252 を出す。249 × 23 = 5727 ≥ 252 × 20 = 5040（pseudo、`jq`：`select(.t_final_us != null and ((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) >= 300)` の数と、その中の `.t_tr_us != null` の数を `rs1`〜`rs3` で数えた。2026-10-20 に 84 / 83、84 / 83、84 / 83 を数えた）。判定の他の理由（対の比較 14 / 19）は、この行が見ない | 陽性 | `the_recorded_rp32_dir_has_enough_translated_samples` |
| T-RP-32-046 | 037 の出力から、訳の取れた割合の列を除いた出力 | 列の欠け | `Err` | `Err` で訳の取れた割合の列を名指す。Rust の割合の列も Python の割合の列も、設計が挙げる列である（specified、RP-32 の「別の列に出す」。024 と 025 の形。列の見出しの綴りは実装の席が決め、test はその綴りを 1 か所に持つ） | 陰性 | `a_missing_translated_share_column_is_rejected` |

**T-RP-32-037 から 046 は未実装。実装の席が足す**（D 3.18 の「測り方の直し」の順 1。製品の code は変えない）。10 本とも fixture を読み、`--ignored` を付けずに通る。`check_alternated_runs` に `check_translated_share`（`native/hayamimi-eval/src/compare.rs`）を Rust の 3 run を合わせた標本へ掛ける 1 行と割合の列が入るまで、今の main で落ちるのは 038（`Ok` になる）、039（載る理由が 3 つで割合が無い）、042、045（割合の列が無い）、044（`Ok` になる）、046（列を調べる関数が無い）である。037、040、041、043 は割合を見ない今の main でも通り、割合を足した後で通ったままであることを固定する側である。
**割合の計算は整数で行う**：`訳の取れた × 23 ≥ 有効 × 20`（`compare.rs` の `SHARE_DENOMINATOR` と `SHARE_NUMERATOR`）。小数の割合を作らない。`compare.rs` の unit test `twenty_of_twenty_three_have_enough_translations_and_nineteen_do_not`（23 標本の 20 と 19）が、この関数の単体の陽性対照で、この節の行は関数を run の判定に掛けた後の値を固定する。
**F32L の作り方**：標本は式で作る。Python の k 番目の run の発話 u の repeat r の標本は `L` 1000 ms、`t_end` 0、`L_asr` 700 ms、復号 300 ms（無音 400 ms）。Rust も同じで、訳の取れない標本は `t_tr` を持たない。実測のふりをしない。

**T-RP-32-030 から 036 は未実装。実装の席が feat/rp32-test に足す。** 7 本とも fixture の dir を読み、`--ignored` を付けずに通る。`pairs.txt` の中身の例は `2` と改行 1 つ（D 3.16 の 4 の「算用数字 1 つを 1 行で」）。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 「訳」の段（`final` から `t_tr` まで）と `L_tr`（`t_tr − t_final`）は、3.1 の定義では同じ値である。設計が 2 つを別の列に挙げる意図が、別の測り方（たとえば中継の log の時刻）なら、その測り方。023 は同じ値を 2 列に出す形で書いた。
- 対の比較の「発話ごとの 5 回の中央値」は、3 対を合わせると 1 発話 15 標本になる。F32 は 1 発話 3 標本（1 回 × 3 対）で、中央値を 3 つから取る。比較集合の「Python の全 run」を 3 つの run のそれぞれと読んだ（021）。
- 後の load は記録だけにした（014）。設計は後の load の上限を書かない。
- 「他の席が cargo の build を打たない」ことは、前の load が 2 未満であることでしか見えない。build が run の途中に始まっても、前の load には出ない。
- 順は Python が先頭（003）。設計は「Python、Rust、Python、Rust … の順」と書く。
- 訳の取れない標本の数を合否に使わない。028 の差は、数え方を直しても Rust が Python を超える。
- dir の中の run と load の file の名前。設計は `pairs.txt` と `load<N>.txt` の名前だけを書く。030 から 036 の fixture の dir は、実装の席が名前を決めて置く。

### 4.43 RP-33（T が V に落ちた行の最初の訳）

test：`native/hayamimi-eval/tests/rp33.rs`。受入 test の名前は `rp33_first_translation_after_a_failed_draft_beats_python`。実走の行（T-RP-33-016）だけが `#[ignore]` を持ち、RP-32 の Rust の run と中継の記録を読む。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp33`。実走の行はこれに `-- --ignored` を足す。
`check_first_translation(rust, python)` は、RP-31 の数え方で出した標本のうち、有効で訳が取れた物の「訳」の段（`final` から `t_tr` まで）を、T が V に落ちた行と通った行に分けて、p50 と p95 を出す。
採る案は 3.16 の 2 のとおり B で、A は 5 つの条件が揃った時にだけ作る。したがって行は 2 つに分かれる。001 から 013 は B のままでも A を作った後でも同じに通る行、014 から 016 は A を作った時にだけ足す行である。

**行の分け方**：T が V を通った行は、窓の最後の `final` と同じ `id` で、`of` が `final` で `stage` が `t`、`status` が `ok` の訳の event が在る行である。中継は先の訳（`stage` が `t`）を、T の訳が V を通った時だけ出す（`pipeline.rs` の `run_staged`。`draft.passed` が真で `of` が `final` の時だけ `early` を呼ぶ）。無い行が、T が V に落ちた行である。
設計は「中継の stdout の行ごとの記録」で分けると書くが、中継の stdout の `translate.line.ok` は `id`、`source`、`t_ms`、`e_ms`、`v_ms`、`stage` を持ち、T の訳が V に落ちたかを持たない。この節は、test が受けた `/events` の `translation` の event（R8e の記録と同じ物）で分ける。

**F33**（式で作る。実測のふりをしない）：Rust の run は有効で訳が取れた 20 行で、10 行が T の訳が V を通った行、10 行が V に落ちた行。「訳」の段は特に書かない限り全行 200 ms。Python の run は 20 行で全行 200 ms。n = 20 のとき nearest-rank の p50 は 10 番目（(50 × 20 + 99) / 100）、p95 は 19 番目（(95 × 20 + 99) / 100）である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-33-001 | F33 そのまま | 全行が Python と同じ | `Ok` | `Ok`（specified、RP-33 の「以下」。等号側） | 境界、陽性 | `equal_translation_stage_passes` |
| T-RP-33-002 | F33 で Rust の 19、20 番目の 2 行（V に落ちた行）を 201 ms にする | p95 の順位に当たる標本 | `Err` | `Err` で p95 だけを名指し。19 番目が 201 ms で Python の 200 ms を超え、p50 の 10 番目は 200 ms（specified、RP-33） | 境界、陰性 | `p95_one_millisecond_over_at_the_19th_row_is_rejected` |
| T-RP-33-003 | F33 で Rust の 20 番目の 1 行だけを 201 ms にする | p95 の順位の次 | `Ok` | `Ok`。19 番目は 200 ms のまま（derived、nearest-rank） | 境界 | `p95_is_decided_by_the_19th_row_only` |
| T-RP-33-004 | F33 で Rust の 10 行を 201 ms にする（11〜20 番目） | p50 の順位の次まで | p50 は `Ok` | p50 の 10 番目は 200 ms で通り、p95 の 19 番目が 201 ms で `Err`（p95 だけを名指し）（derived、nearest-rank） | 境界 | `ten_slower_rows_leave_the_p50_alone` |
| T-RP-33-005 | F33 で Rust の 11 行を 201 ms にする（10〜20 番目） | p50 の順位に当たる標本 | `Err` | `Err` で p50 と p95 の両方を名指し（specified、RP-33） | 境界、陰性 | `p50_one_millisecond_over_at_the_10th_row_is_rejected` |
| T-RP-33-006 | F33 で V に落ちた 10 行の最初の訳を 600 ms 遅らせる（800 ms） | 偽の中継（RP-33 の陰性欄） | `Err` | `Err` で p95 を名指し。10 番目までが 200 ms の 10 行で p50 は通り、19 番目が 800 ms。V に落ちた行の列は n = 10、p50 800 ms、p95 800 ms（specified、RP-33 の陰性欄。値は F33 の式） | 陰性 | `six_hundred_ms_slower_failed_drafts_are_rejected` |
| T-RP-33-007 | F33 | 列の出力 | 2 つの列 | V に落ちた行の列が n = 10、p50 200 ms、p95 200 ms。V を通った行の列が n = 10、p50 200 ms、p95 200 ms（specified、RP-33 の「別の列に出す」と陽性欄「T が V を通った行だけの列も出せる」） | 陽性 | `failed_and_passed_draft_columns_are_written_apart` |
| T-RP-33-008 | 007 で V に落ちた行の列が無い出力 | 列の欠け | `Err` | `Err` で V に落ちた行の列を名指し（specified、RP-33） | 陰性 | `a_missing_failed_draft_column_is_rejected` |
| T-RP-33-009 | F33 に、V に落ちて訳が `held` で出なかった行（訳の取れない標本）を 4 行足す | 訳の無い行 | n = 10 のまま | V に落ちた行の列の n は 10 のまま。訳の無い行は p50 と p95 にも n にも入らない（specified、3.1 の「訳の取れない有効な標本の数は…別々に書く」。RP-31 の 001 と同じ除き方） | 境界、陰性 | `rows_without_a_shown_translation_are_left_out_of_the_columns` |
| T-RP-33-010 | RP-31 の A の発話 2（`mic-3` の `stage` が `t` の訳が在る）、B の発話 13（`mic-38` は `stage` が `e` の訳だけ）、A の発話 1（`mic-1` は `held` で `stage` が `e` だけ） | 行の分け方 | 通った、落ちた、落ちた（訳なし） | 発話 2 は V を通った行。発話 13 は V に落ちた行で訳が在る。発話 1 は V に落ちた行で訳が無い。`stage` が `t` の速報の訳（`of` が `partial`）は分けるのに使わない（derived、`pipeline.rs` の `run_staged`。jq で数えた R8e の記録：訳の取れた旧い 93 行のうち `stage` が `t` の訳が無い行が 45、設計の「93 行中 45 行」と一致） | 陽性、陰性 | `a_row_without_a_t_draft_event_is_a_failed_draft` |
| T-RP-33-011 | 010 の `mic-3` の `stage` が `t` の訳の `of` を `partial` にした物 | 速報の `t` で分けない | 落ちた | 発話 2 は V に落ちた行になる。`of` が `partial` の `stage` が `t` の訳は、窓の最後の `final` の先の訳ではない（specified、3.16 の「先の訳は T が V を通った時だけ出す」は `final` の分） | 境界、陰性 | `a_partial_t_translation_does_not_make_a_passed_draft` |
| T-RP-33-012 | R8e の全記録（`docs/results/r8e/` を RP-31 の 018 の数え方で。file が無ければその path を名指しして `Err`）と、R0 の Python の run（`lat_ja_0.30_1`）。有効の判定は 300 ms | 実物の記録 | 表の値 | Rust の「訳」の段は全行 n 66、p50 313 ms、p95 1237 ms。V を通った行 n 47、p50 260 ms、p95 439 ms。V に落ちた行 n 19、p50 701 ms、p95 1526 ms。Python は n 95、p50 212 ms、p95 1019 ms。全行の列は p50 と p95 のどちらも Python を超え、判定は `Err`（pseudo、jq の式。V に落ちた行の n は、訳が取れた行だけを数える。2026-10-14 の記録が示す差を、判定の道具が「超える」と読む確かめ） | 陽性、陰性 | `the_recorded_r8e_failed_drafts_are_slower_than_python` |
| T-RP-33-013 | 012 の Rust の 3 つの列の合計 | 母数 | 47 + 19 = 66 | V を通った行 47 と落ちた行 19 の和が全行 66 と等しい。どちらにも入らない有効な行を作らない（derived、分け方の定義） | 境界 | `passed_and_failed_rows_add_up_to_all_rows` |
| T-RP-33-014 | A を作った時だけ。A の前の run の V を通った行の `L` が 1000〜1009 ms の 10 行（n = 10 で p50 は 5 番目、p95 は 10 番目）。A の後の run が同じ値 | V を通った行の `L`（等号） | `Ok` | `Ok`（specified、RP-33 の「A を作った時は…A の前の run 以下」） | 境界、陽性 | `a_passed_draft_l_equal_to_before_a_passes` |
| T-RP-33-015 | 014 で A の後の run の 5 番目の行（1004 ms）を 1005 ms にする。10 番目の行を 1010 ms にした別の入力も作る | p50 の順位、p95 の順位 | `Err` | 5 番目が 1005 ms になる入力は `Err` で p50 を名指し。10 番目が 1010 ms になる入力は `Err` で p95 を名指し。1 番目だけを 1001 ms にした入力は `Ok`（specified、RP-33） | 境界、陰性 | `a_slower_passed_draft_l_after_a_is_rejected` |
| T-RP-33-016 | 実走：RP-32 の Rust の run の `/events`（B の後の run）。A を作った後は A の前の run も | 実走 | 実走 | 001 の形で、全行の「訳」の段が Python 以下（specified、RP-33） | 陽性 | `rp33_first_translation_after_a_failed_draft_beats_python` |

A を作る条件（3.16 の 2）のうち、ID の合否に入る物は 014 と 015 だけである。残りの 3 つは A を作る時の前提で、測り方は次の表のとおり（A を作った時だけ足す。今は走らせない）。

| 前提 | 測り方 | 閾値 | 期待値 |
|---|---|---|---|
| H6 が通る | R8e の branch の `native/hayamimi-relay/tests/r8e_server.rs` の `closing_the_stream_after_the_first_token_frees_the_slot_within_a_second` と `an_open_stream_keeps_its_slot_busy_until_the_answer_ends`（`--ignored`、port 8080 の slot 4 本の llama-server） | 1 秒以内 | 閉じた後の slot が 1 秒以内に空き、閉じない対照では生成の終わりまで埋まる（R8e の記録は 44 ms） |
| E0 にも V を掛け、閾値を動かさずに RP-35 を通す | RP-35 の 012 から 018 を A の build で打つ | RP-35 のとおり | RP-35 のとおり |
| E0 が答えなければ直列の E へ落とし、3.15 の形で event を出す | E0 に答えない偽の llama-server で 1 行を流し、`/events` と中継の log を読む | event 1 件 | 行は `ok` で出て、3.15 の形の `fallback` の event と log が 1 件。event の名前と `reason` は設計に無い |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- T の訳が V に落ちたことを持つ記録。中継の stdout にも `/events` にも「落ちた」を直接示す欄は無く、この節は先の訳の event が無いことから導いた。落ちた理由は中継が捨てている（`draft()` の `passed` が真偽だけを持つ）。B の「`hold_reason` ごとに数える」ための欄は、B の席が足す。
- B の後の run であること。run がどの build かを示す欄の名前が設計に無い。B の前の run を渡しても、001 から 013 は同じに通ったり落ちたりし、見分けられない。
- A を作った時の slot 4 本の収まり（2 音源が同時に話した時の T 2 本、E0 2 本、速報、E）。同時に走った依頼の数を示す記録が中継にも llama-server の log にも設計に無く、数えられない。
- E0 が答えない時の event の名前と `reason`。

### 4.44 RP-34（確定の復号の前の待ち）

test：`native/hayamimi-eval/tests/rp34.rs`。受入 test の名前は `rp34_wait_before_final_decode_not_longer_than_python`。実走の行（T-RP-34-017）だけが `#[ignore]` を持ち、RP-32 の run と、待ちの log と、各 run の前後の `nvidia-smi` の記録を読む。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp34`。実走の行はこれに `-- --ignored` を足す。
合否の列は `W = L_asr − decode_ms` である。復号の順は変えないので（3.16 の 3）、案の分かれる行は無い。
`check_wait(rust, python)` は、有効な標本（RP-3 の分割式で有効）ごとに、窓の最後の `final` の `audio_s` で組を決める。4 秒未満、4 秒以上 7 秒未満、7 秒以上である。組ごとに `W` の p50 を出し、Rust が Python 以下かを比べる。
`W` は µs の整数（`L_asr` は µs、`decode_ms` は 10 の -3 乗 ms の整数に四捨五入）で比べる。

**F34**（式で作る。実測のふりをしない）：組ごとに標本 n 個、i = 1〜n。`decode_ms` は 100 + i、`L_asr` は 900 + 2i ms（したがって `W` は 800 + i ms）。4 秒未満は n = 12（`audio_s` 2.0）、4〜7 秒は n = 13（5.0）、7 秒以上は n = 11（8.0）。Rust は書いた箇所のほかは Python と同じ。p50 の順位は n = 12 で 6 番目、n = 13 で 7 番目、n = 11 で 6 番目（(50 × n + 99) / 100）。したがって `W` の p50 は 806、807、806 ms。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-34-001 | `audio_s` が 3.999、4.000、6.999、7.000 の標本 | 組の境界 | 4 秒未満、4〜7 秒、4〜7 秒、7 秒以上 | 4.000 は 4〜7 秒の組、7.000 は 7 秒以上の組（specified、RP-34 の「4 秒未満、4〜7 秒、7 秒以上」） | 境界 | `group_edges_put_4_seconds_in_the_middle_and_7_seconds_in_the_top` |
| T-RP-34-002 | F34 で 7 秒以上の標本が 0 個 | 組が空 | `Err` | `Err` で 7 秒以上の組を名指し。空の組を通過と読まない（implicit） | 境界、陰性 | `an_empty_group_is_an_error_not_a_pass` |
| T-RP-34-003 | F34 そのまま | 等号 | `Ok` | `Ok`（specified、RP-34 の「Python 以下」） | 境界、陽性 | `equal_waits_pass` |
| T-RP-34-004 | F34 で Rust の 4 秒未満の組の 6 番目の `L_asr` を +1 µs | p50 の順位（4 秒未満） | `Err` | `Err` で 4 秒未満を名指し。806.001 ms が 806 ms を超える（specified、RP-34） | 境界、陰性 | `short_group_one_microsecond_over_at_the_6th_sample_is_rejected` |
| T-RP-34-005 | 004 を 7 番目の標本にする | p50 の順位の次 | `Ok` | `Ok`（derived、nearest-rank） | 境界 | `short_group_wait_is_decided_by_the_6th_sample_only` |
| T-RP-34-006 | F34 で Rust の 4〜7 秒の組の 7 番目の `L_asr` を +1 µs | p50 の順位（中） | `Err` | `Err` で 4〜7 秒を名指し（specified、RP-34） | 境界、陰性 | `middle_group_one_microsecond_over_at_the_7th_sample_is_rejected` |
| T-RP-34-007 | F34 で Rust の 7 秒以上の組の 6 番目の `L_asr` を +1 µs | p50 の順位（上） | `Err` | `Err` で 7 秒以上を名指し（specified、RP-34） | 境界、陰性 | `top_group_one_microsecond_over_at_the_6th_sample_is_rejected` |
| T-RP-34-008 | F34 で Rust の 3 つの組のそれぞれ 1 番目の `L_asr` を −1 µs | 下側 | `Ok` | `Ok`（specified、RP-34） | 境界 | `a_shorter_wait_passes` |
| T-RP-34-009 | F34 で Rust の `decode_ms` を全標本 2 倍にし、`L_asr` を `W` が同じになるように足す。待ちの log の p50 は 5000 ms | 記録の列は合否に使わない | `Ok` | `Ok`。`decode_ms` の p50 と待ちの log の p50 は記録の列に出て、判定に入らない（specified、RP-34） | 境界 | `decode_ms_and_the_wait_log_are_recorded_but_do_not_decide` |
| T-RP-34-010 | F34 で Rust の全標本の `L_asr` を +300 ms | 確定の復号の前に 300 ms 待つ偽の engine | `Err` | `Err` で 3 つの組すべてを名指し（specified、RP-34 の陰性欄「3 つの組のどれかが落ちる」。この入力は全組に 300 ms が乗る） | 陰性 | `a_300_ms_wait_before_the_final_decode_is_rejected` |
| T-RP-34-011 | 3 対の GPU の記録（前後の `utilization.gpu` と `memory.used`）が Python と Rust で同じ | 負荷が同じ対 | 無効な対 0 | `Ok`。無効にした対の数 0 が列に出る（数えて 0 と数えていない 0 を分ける）（specified、RP-34） | 陽性 | `pairs_with_the_same_gpu_load_are_valid_and_the_invalid_count_is_zero` |
| T-RP-34-012 | 1 つの対で、Python の記録が `0, 9803`、Rust の記録が `95, 15000`（`utilization.gpu` の %、`memory.used` の MiB） | 負荷が大きく違う対 | 無効な対 1 | その対は判定から除き、無効にした対の数 1 が出る（specified、RP-34 の「違う対は無効にし、その数を出す」。「違う」の幅は設計に無いので、どの幅を置いても違う入力を選んだ） | 陰性 | `a_pair_with_very_different_gpu_load_is_invalidated_and_counted` |
| T-RP-34-013 | 012 で Rust の run の GPU の記録が無い | 記録の欠け | `Err` | `Err` で run を名指し。値は出さない（specified、RP-34 の陰性欄「GPU の記録が無い run」） | 陰性 | `a_run_without_a_gpu_record_is_rejected` |
| T-RP-34-014 | F34 の Rust の run で、速報を切った run（`partial` の event が 0 件）。待ちの log は在る | 速報を切った run | 列が出る | 3 つの組の `W` の p50 と `decode_ms` の p50 と待ちの log の p50 が出る（specified、RP-34 の陽性欄）。速報の遅れの判定は掛けない | 陽性 | `a_run_without_partials_still_yields_the_columns` |
| T-RP-34-015 | F34 の run に、速報の遅れ（RP-5 の `first_partial_delay`）の p95 が Python より 1 ms 大きい Rust の run | 速報は RP-5 を満たす | `Err` | `Err` で RP-5 を名指し。`W` の判定は通る（specified、RP-34 の「速報は RP-5 を満たす」。RP-5 の 002 と同じ関数） | 境界、陰性 | `partial_delay_over_rp5_is_rejected_even_when_the_wait_passes` |
| T-RP-34-016 | R8e の Rust の run（RP-31 の 018 の数え方）と R0 の Python の run（`lat_ja_0.30_1`）。有効の判定は 300 ms | 実物の記録 | 表の値 | `W` の p50 は、4 秒未満が Rust 884.706 ms、Python 884.021 ms（n 12 と 12）、4〜7 秒が Rust 990.724 ms、Python 946.288 ms（n 65 と 65）、7 秒以上が Rust 902.800 ms、Python 860.555 ms（n 23 と 23）。3 つの組のどれも Rust が超え、`Err`（pseudo、jq。Python の `decode_ms` の p50 は 111.0、182.7、248.1 ms。旧い数え方の Rust の `decode_ms` の p50 は 4〜7 秒が 274.6 ms、7 秒以上が 440.1 ms で、3.16 の 275 ms と 440 ms に一致する） | 陰性 | `the_recorded_r8e_wait_is_longer_than_python_in_all_three_groups` |
| T-RP-34-017 | 実走：RP-32 の Rust と Python の run と、待ちの log と、前後の `nvidia-smi` の記録 | 実走 | 実走 | 3 つの組のそれぞれで Rust の `W` の p50 が Python 以下。無効にした対の数を出す（specified、RP-34） | 陽性 | `rust_wait_before_final_decode_is_not_longer_than_python` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- GPU の負荷が「違う」対の幅。`utilization.gpu` の %、`memory.used` の MiB のどちらに何を置くか。幅を境にした行（境界の両側）は、決まるまで書けない（011 と 012 は極端な入力で、幅の値に依らない）。
- 全部の対が無効になった時の扱い。
- 「発話の長さ」は窓の最後の `final` の `audio_s` で読んだ（R8e の記録の `decode_ms` の p50 が 3.16 の 275 ms と 440 ms に一致する組分け）。
- 待ちの log の event の名前と欄。014 と 009 は意味の名前で書いた。
- `decode_ms` の意味。Python の `decode_ms` が待ちを含むかは確かめられていない（10 の H43）。016 は両方の `decode_ms` を同じ欄として引いた。

### 4.45 RP-35（`L` の詰めで品質を下げない）

test：`native/hayamimi-eval/tests/rp35.rs`。受入 test の名前は `rp35_latency_work_keeps_chrf_and_verify`。実走の行（T-RP-35-021）だけが `#[ignore]` を持ち、前の main と R8h の branch の `eval` の結果と、RP-32 の Rust の run の中継の記録を読む。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp35`。実走の行はこれに `-- --ignored` を足す。
`check_quality_kept(before, after, v_records)` は、2 組の `eval --final` の結果（方向ごとに `heldout_sha256`、T と E の model の名前、行の数、V が止めた行の数、chrF）と、RP-13 の en の chrF、RP-32 の run の `status` が `ok` の訳の列と V の記録の列を受ける。
chrF は 10 の -6 乗の整数に読んで比べる（4.1）。held-out の chrF の出し方（`eval` の結果の file の欄か、参照訳と hyp から RP-14 の関数で出すか）は設計に無いので、test は値を受ける関数の形にする。

**F35**（式で作る。実測のふりをしない）：前の main の結果は、ja→en が行 40、止めた行 2、chrF 40.000000。en→ja が行 40、止めた行 1、chrF 30.000000。RP-13 の en の chrF は 47.570000。model の名前と `heldout_sha256` は前後で同じ。R8h の結果は、書いた箇所のほかは前の main と同じ。
V の記録は、translation の `id`、`stage`、V に掛けた文字列の sha256、判定（`ok` か止めた）の組である。記録の欄の名前は設計に無く、実装の席が決める（今の中継の stdout には `ok` の記録が無い。`translate.line.ok` は `id`、`source`、`t_ms`、`e_ms`、`v_ms`、`stage` だけを持つ）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-35-001 | F35 そのまま | 全部が等号 | `Ok` | `Ok`（specified、RP-35 の「以上」「以下」の等号側） | 境界、陽性 | `equal_results_pass` |
| T-RP-35-002 | F35 で R8h の ja→en の chrF を 39.999999 にする | 最小の桁だけ下 | `Err` | `Err` で ja→en の chrF を名指し（specified、RP-35） | 境界、陰性 | `ja_en_chrf_one_micro_below_is_rejected` |
| T-RP-35-003 | F35 で R8h の en→ja の chrF を 29.999999 にする | 方向ごとに独立 | `Err` | `Err` で en→ja の chrF を名指し。ja→en は通る（specified、RP-35 の「両方の向きで」） | 境界、陰性 | `en_ja_chrf_one_micro_below_is_rejected` |
| T-RP-35-004 | F35 で R8h の chrF を両方とも +0.000001 | 上側 | `Ok` | `Ok`（specified、RP-35） | 境界 | `slightly_better_chrf_passes` |
| T-RP-35-005 | F35 で R8h の ja→en の止めた行を 3 にする | 止める行が増える | `Err` | `Err` で ja→en の止めた行を名指し（specified、RP-35 の「止める行が増えない」） | 境界、陰性 | `one_more_stopped_row_is_rejected` |
| T-RP-35-006 | F35 で R8h の ja→en の止めた行を 1 にする | 止める行が減る | `Ok` | `Ok`（specified、RP-35 の「前の main 以下」） | 境界 | `one_fewer_stopped_row_passes` |
| T-RP-35-007 | F35 で R8h の RP-13 の en の chrF を 47.569999 にする | RP-13 の en の行 | `Err` | `Err` で RP-13 の en を名指し（specified、RP-35 の「RP-13 の en の chrF が前の main 以上」） | 境界、陰性 | `rp13_en_chrf_one_micro_below_is_rejected` |
| T-RP-35-008 | F35 で R8h の RP-13 の en の chrF を 47.570000（等号）にして、ほかを F35 のままにする | RP-13 の en の等号 | `Ok` | `Ok`（specified、RP-35） | 境界、陽性 | `rp13_en_chrf_equal_passes` |
| T-RP-35-009 | F35 で R8h の `heldout_sha256` を前の main と違う値にする | 同じ held-out でない | `Err` | `Err` で `heldout_sha256` を名指し。比べない（derived、RP-35 の「同じ llama-server と model」の前提の同じ組） | 陰性 | `different_heldout_batches_are_not_compared` |
| T-RP-35-010 | F35 で R8h の E の model の名前を前の main と違う値にする | 同じ model でない | `Err` | `Err` で model を名指し（specified、3.16 の「同じ server、同じ model」） | 陰性 | `different_models_are_not_compared` |
| T-RP-35-011 | F35 で R8h の ja→en の行を 39 にする | 行の数が違う | `Err` | `Err` で行の数を名指し。行が減った結果の止めた行の数を比べない（derived） | 境界、陰性 | `different_row_counts_are_not_compared` |
| T-RP-35-012 | RP-32 の Rust の run の fixture：`status` が `ok` の訳 4 件（`stage` が `t` の先の訳が 2 件、`e` が 2 件）。4 件とも、同じ `id` と `stage` と訳の文字列の sha256 の V の記録が `ok` で在る | V の記録が全件に在る | `Ok` | `Ok`（specified、RP-35 の「`status` が `ok` の訳の全件に、その訳の文字列に掛けた V の `ok` の記録が在る」） | 陽性 | `every_ok_translation_has_its_own_ok_verify_record` |
| T-RP-35-013 | 012 で 1 件の `ok` の訳（`stage` が `e`）の V の記録を除く | V を掛けずに出した訳（E0 か E の応答） | `Err` | `Err` でその訳の `id` と `stage` を名指し（specified、RP-35 の陰性欄） | 陰性 | `an_ok_translation_without_a_verify_record_is_rejected` |
| T-RP-35-014 | 012 で 1 件の訳の V の記録を、別の文字列（その `id` の先の訳の文字列）の sha256 にする | 記録が別の文字列に掛けた物 | `Err` | `Err` でその訳を名指し。訳の文字列の sha256 と記録の sha256 が一致しなければ記録と数えない（specified、RP-35 の「その訳の文字列に掛けた」） | 境界、陰性 | `a_verify_record_for_another_text_does_not_count` |
| T-RP-35-015 | 012 で 1 件の V の記録の判定を「止めた」にする | 止めた訳が出ている | `Err` | `Err` でその訳を名指し（specified、RP-35 の「V の `ok` の記録」） | 陰性 | `an_ok_translation_whose_verify_record_is_a_stop_is_rejected` |
| T-RP-35-016 | 012 に `status` が `held` の訳と `unavailable` の訳を 1 件ずつ足す。V の記録は無い | 出ない訳 | `Ok` | `Ok`。V の記録の対象は `ok` の訳だけ（specified、RP-35） | 境界 | `held_and_unavailable_translations_need_no_ok_record` |
| T-RP-35-017 | 012 に `of` が `partial` で `status` が `ok` の訳を 1 件足す。V の記録は無い | 速報の訳 | `Err` | `Err` でその訳を名指し。速報の訳も `status` が `ok` の訳で、V を通った物だけが出る（specified、RP-35 の「全件」と 3.6 の「速報の訳にも V を掛け」） | 境界、陰性 | `a_partial_translation_without_a_verify_record_is_rejected` |
| T-RP-35-018 | F35 で R8h の ja→en の chrF を 38.5 にする | T の訳をそのまま確定にする偽の中継 | `Err` | `Err` で ja→en の chrF を名指し。en→ja は通る（specified、RP-35 の陰性欄。値は F35 の式で、実測ではない） | 陰性 | `t_draft_used_as_final_lowers_chrf_and_is_rejected` |
| T-RP-35-019 | 前の main の結果を 2 組（同じ内容）渡す | 前の main 同士 | 差 0 | `Ok` で、chrF、止めた行、RP-13 の en のどの差も 0（specified、RP-35 の陽性欄） | 陽性 | `identical_baselines_give_zero_difference` |
| T-RP-35-020 | 待ちの log を入れた build と分かる印が在る結果。ja の CER が前の main 0.038、R8h 0.038 | RP-1 の ja の CER | `Ok` | `Ok`。R8h の ja の CER を 0.039 にすると `Err`（specified、3.16 の「待ちの log は RP-1 の ja の CER と RP-5 の速報の遅れを悪くしない」。RP-1 の最小の桁は 0.001） | 境界 | `a_worse_ja_cer_after_the_wait_log_is_rejected` |
| T-RP-35-021 | 実走：前の main と R8h の branch で、同じ llama-server と model に `eval --final --heldout <path> --direction ja-en` と `--direction en-ja`、RP-13 の en の行を打つ。RP-32 の Rust の run の中継の記録 | 実走 | 実走 | 001 の形で `Ok`（specified、RP-35）。held-out は 1 batch につき 1 回で、表が同じ `tables_sha256` の 2 回目だけを走らせる（会話字幕の CS-30） | 陽性 | `r8h_keeps_chrf_and_every_shown_translation_passed_v` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- V の `ok` の記録の欄の名前。今の中継は `ok` の判定を log にも `/events` にも出さないので、012 から 017 の実走（021）は、記録を出す変更が先に要る。
- 待ちの log を入れた build と分かる印（020）。
- held-out の chrF の出所（結果の file の欄か、test が計算か）。held-out の参照訳は repo の外にあり、test の席が読めるのは `eval` の結果だけである。
- LLM の出力が run ごとに揺れる場合の扱い。「前の main 以上」は 1 回ずつの比較で、同じ入力でも揺れれば落ちたり通ったりする。揺れの幅は未測定である。

### 4.46 RP-36（RP-1 の ko の R11 の入口の上限）

test：`native/hayamimi-eval/tests/rp36.rs`。受入 test の名前は `rp36_ko_error_within_the_shipped_build_cap`（001）。001 だけが `#[ignore]` を持ち、配る形の build の ko の記録を読む。ほかの行は `--ignored` を付けずに通る。
関数の名前は `rp36_` に各行の「関数の末尾」を続けた物にする。
入力：`docs/results/r3c/win_routed.json`（配る形の build の run。ko の行は `mean_err_micros` 86939、`clips` 12）、`docs/results/r3c/win_python_libs.json`（Python の wheel の ORT を読ませた run。ko の行は 80572、`clips` 12）、`native/hayamimi-eval/tests/fixtures/` の `rp36_ko_86940.json`（ko の 1 行だけ、86940）、`rp36_ko_86939.json`（同じ形、86939）、`rp36_zh_over_cap_ko_within.json`（ja 37512、zh 90000、ko 80572 の 3 行）。後ろの 2 つは新しく置く。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp36`。001 は同じ command に `-- --ignored` を足す。
`check_ko_cap(card)` は ko の行を探し、`mean_err_micros`（10 の -6 乗の整数）を 86939 と整数のまま比べる。比べる前に丸めない。RP-1 の `check_scorecard`（小数 3 桁）には触れない。86939 を 3 桁に丸めた 0.087 は RP-1 の閾値 0.081 を超えるが、RP-36 は 86939 を通す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-36-001 | `win_routed.json`（配る形の build の run） | 上限の等号側。実記録 | 86939 以下 | `Ok(86939)`（specified、RP-36 の「86939 以下」。86939 は 10 の H35 の記録値） | 境界、陽性 | `ko_error_within_the_shipped_build_cap` |
| T-RP-36-002 | `win_python_libs.json`（Python の wheel の ORT の run） | 上限より下。ORT が違う run | 86939 以下 | `Ok(80572)`（specified、RP-36 の陽性欄「ko 80572」） | 陽性 | `python_wheel_run_reads_ko_80572` |
| T-RP-36-003 | fixture `rp36_ko_86940.json` | 上限の 1 つ上（最小の桁 1 つ分） | 86939 以下 | `Err(OverCap(86940))`（specified、RP-36 の陰性欄「ko の平均を 86940 にした `rp01.json`」） | 境界、陰性 | `ko_86940_is_over_the_cap` |
| T-RP-36-004 | 002 の記録から ko の行を除いた物 | 行の欠け | `Err` | `Err(NoKoRow)`。ko の行が無い記録を通過と読まない（implicit） | 境界、陰性 | `a_record_without_a_ko_row_is_refused` |
| T-RP-36-005 | fixture `rp36_ko_86939.json` | 上限の等号側。実記録に頼らない | 86939 以下 | `Ok(86939)`。001 は `#[ignore]` で通常の run に入らないので、003 の対の側を `--ignored` なしで固定する（specified、RP-36） | 境界、陽性 | `ko_86939_is_within_the_cap` |
| T-RP-36-006 | fixture `rp36_zh_over_cap_ko_within.json` | ko 以外の行が上限を超える | 86939 以下 | `Ok(80572)`。比べるのは ko の行だけ（specified、RP-36 は ko の平均の上限） | 境界 | `only_the_ko_row_is_compared` |

実装済みの行は 001 から 004、未実装の行は 005 と 006 である（実装の席が足す）。
陰性の build（ko の平均を 86940 にした `rp01.json`）で落ちる行は 003 である。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 001 が読む file。6 の RP-36 の行は RP-1 の run が書く `rp01.json` を読むと書く。001 は commit 済みの `docs/results/r3c/win_routed.json` を読む。R11 の入口で上限を打つ時に、commit 済みの記録と、その時の配る形の build の `rp01.json` のどちらを読むかは設計が決める。
- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書くが、実装済みの test は `rp36_` + 末尾である。この節は実装に合わせた。
- ko の行の `clips` が 0 の記録の扱い。`mean_err_micros` が 0 でも測っていない。設計に条件が無いので行にしない。
- 「超えたら評議へ出す」は command で測れない（持ち主は受付）。

### 4.47 RP-37（zh、ko、es の V の推量を log にだけ残す）

test：`native/hayamimi-relay/tests/rp37.rs`。受入 test の名前は `rp37_hedge_mismatch_is_logged_in_zh_ko_es_and_held_in_ja_en`（001 から 004）。名前の `held_in_ja_en` は、003 と 009 が止めずに log する向きになり、止まるのは 004（en→ja で原文の推量を落とした訳）だけなので、内容と食い違う。名前を変えるかは実装の席が決める。`#[ignore]` を持たない（偽の llama-server の応答だけを使う）。
関数の名前は `rp37_` に各行の「関数の末尾」を続けた物にする。001 から 004 は受入 test の名前の 1 本が順に確かめ、失敗の文は言語の code を前置きにする。
入力：原文は 3 つ。`HEDGED`＝`明日は雨かもしれません。`（推量の語 `かもしれ`）、`PLAIN`＝`明日は雨です。`（推量の語も否定の語も無い）、`NEGATED_SOURCE`＝`そこにいませんでした。`（否定の語 `ません`）。偽の llama-server は全部の要求に決めた 1 つの文で答える。zh、ko、es は T だけなので要求は 1 回で、ja と en の組は T と E が同じ文を受ける。語の表は `native/hayamimi-relay/data/verify_markers.toml`。
command（001 から 017）：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp37`。
018 以降は cargo の test ではなく、書いていない検証の席が打つ。vprobe の入力は dev の組（`testdata/baseline/pairs/` が seed 0、`testdata/baseline/pairs_seed1/` が seed 1）だけで、`testdata/heldout/test_split/` は渡さない。`<main>` と `<branch>` は、今の main と branch をそれぞれ build し、binary を別の名前で控えた物を指す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-37-001 | `HEDGED` を zh、ko、es へ。答えは推量の語が無い文（zh `明天会下雨。`、ko `내일은 비가 와요.`、es `Lloverá mañana.`）。言語ごとに 1 回 | 原文だけが推量 | 止まらずに出る | `Ok`、出る訳が答えと同じ、`translate.hedge.logged` が 1 件で `id` が要求の `id`、`lang` が言語の code、原文の推量の語が文字列、訳の推量の語が null（specified、RP-37 の期待値と 3.6 の R8i） | 陽性 | `hedge_mismatch_is_logged_in_zh_ko_es_and_held_in_ja_en` |
| T-RP-37-002 | `PLAIN` を zh、ko、es へ。答えは推量の語を持つ文（zh `明天可能会下雨。`、ko `내일은 비가 올 수도 있어요.`、es `Quizás llueva mañana.`） | 訳だけが推量（001 の逆向き） | 止まらずに出る | `Ok`、log が 1 件、原文の推量の語が null、訳の推量の語が文字列（specified、RP-37。語の表は `verify_markers.toml`） | 陽性 | 001 と同じ 1 本 |
| T-RP-37-003 | ja から en。`HEDGED`、T と E の答えは `It will rain tomorrow.` | ja と en の組。推量が原文だけ。訳の向きが en | 止まらずに出る | `Ok`、出る訳が答えと同じ、`translate.hedge.logged` が 1 件（T と E の両方が通っても、確定の訳の 1 回だけ）で `lang` が `en`、原文の推量の語が `かもしれ`、訳の推量の語が null（specified、`rust_port.ja.md` の 6 の RP-48 と 8 の R8s の 1） | 陽性 | 001 と同じ 1 本 |
| T-RP-37-004 | en から ja。`It might rain tomorrow.`、答えは `明日は雨が降ります。` | 003 と向きが逆 | 止まる | `Held`、理由 `uncertainty`、log 0 件（specified、RP-37） | 陰性 | 001 と同じ 1 本 |
| T-RP-37-005 | zh、ko、es のそれぞれで、`HEDGED` へ推量を残した訳（002 の 3 文）と、`PLAIN` へ推量の無い訳（001 の 3 文） | 食い違いが無い 2 通り（両方に有る、両方に無い） | 止まらずに出る | `Ok`、log 0 件（specified、RP-37 の陽性欄「食い違いの無い訳を止めずに出し、`translate.hedge.logged` が 0 件」） | 陽性、log の陰性 | `agreeing_hedges_log_nothing` |
| T-RP-37-006 | `NEGATED_SOURCE` を zh、ko、es へ。落とした訳（zh `我在那里。`、ko `저는 거기에 있었어요.`、es `Estaba allí.`）と、残した訳（zh `我没有在那里。`、ko `저는 거기에 없었어요.`、es `No estaba allí.`） | 否定の食い違い。残した訳が対 | 落とした訳は止まる | 落とした訳は `Held`、理由 `negation`、log 0 件。残した訳は `Ok`（specified、RP-37 の「否定を落とした訳は `negation` で止まる」） | 陰性、陽性 | `a_lost_negation_still_holds_in_zh_ko_es` |
| T-RP-37-007 | `予算は五百万円です。` を zh、ko、es へ。答えは数を変えた文（zh `预算大约是7百万日元。`、ko `예산은 7백만 엔 정도예요.`、es `El presupuesto es de 7 millones de yenes.`） | 数の食い違い。zh と ko は推量の語も付き、es は付かない | 止まる | `Held`、理由 `number`、log 0 件。推量の食い違いが同居しても理由は数（specified、RP-37 の「数を変えた訳は数の理由で止まる」） | 陰性 | `a_changed_number_still_holds_in_zh_ko_es` |
| T-RP-37-008 | ja から en。`PLAIN`、答えは `It might rain tomorrow.` | 003 の逆向き。推量が訳だけ | 止まらずに出る | `Ok`、出る訳が答えと同じ、`translate.hedge.logged` が 1 件、原文の推量の語が null、訳の推量の語が `might`（specified、RP-48 と R8s の 1） | 陽性 | `an_added_hedge_into_en_is_logged_and_not_held` |
| T-RP-37-009 | en から ja。`It will rain tomorrow.`、答えは `明日は雨かもしれません。` | 004 の逆向き。推量が訳だけ | 止まらずに出る | `Ok`、`translate.hedge.logged` が 1 件（specified、6 の RP-63 の「原文が推量を持たず訳だけが持つ行は止まらずに出て、`translate.hedge.logged` が 1 件出る」。T-RP-63-002 と同じ入力で、実装の席は 1 本にまとめてよい） | 陽性 | `an_added_hedge_into_ja_is_logged_and_not_held` |
| T-RP-37-010 | `そこにいないかもしれません。`（否定 `ない` と推量 `かもしれ` の両方）を zh、ko、es へ。答えは否定も推量も持たない文（zh `他在那里。`、ko `그는 거기에 있어요.`、es `Él está allí.`） | 否定の食い違いと推量の食い違いが同居 | 止まる | `Held`、理由 `negation`、log 0 件。止めた行は推量を log に残さない（specified、RP-37 の「否定を落とした訳は `negation` で止まる」。log の対象は止めずに出した訳） | 境界、陰性 | `a_lost_negation_wins_over_a_hedge_mismatch_in_zh_ko_es` |
| T-RP-37-011 | `PLAIN` を zh、ko、es へ。答えは否定を足した文（zh `明天不会下雨。`、ko `내일은 비가 안 와요.`、es `No lloverá mañana.`） | 否定が訳だけ（006 の逆向き） | 止まる | `Held`、理由 `negation`、log 0 件（specified、3.6 の「否定の検査は今のまま」。表は有無の一致を見る） | 陰性 | `an_added_negation_holds_in_zh_ko_es` |
| T-RP-37-012 | `予算は五百万円です。` を zh、ko、es へ。答えは数が同じ文（zh `预算是500万日元。`、ko `예산은 5,000,000엔이에요.`、es `El presupuesto es de 5000000 yenes.`。`native/hayamimi-relay/tests/rp37.rs` の入力と同じ） | 数の食い違いが無い（007 の対） | 止まらずに出る | `Ok`、log 0 件（specified、RP-37。007 の陰性対照）。数の書き方は今の `numbers.rs` が読む形にする。倍の語の形（`5百万`、`5백만`、`5 millones`）は RP-39 の case が持つ | 陽性 | `the_same_number_passes_in_zh_ko_es` |
| T-RP-37-013 | 001 の ko の 1 件の log | log の形 | 欄の集合 | `level` が info。欄は `id`、`lang`、原文の推量の語、訳の推量の語の 4 つ。原文の語は `かもしれ`。`message` と全部の欄の値に、原文の文と訳の文が入らない（specified、3.6 の「項目は `id`、`lang`、原文と訳の推量の語」と `observability-and-log-monitoring.md` の「入力本文を出力しない」）。実装の欄の key は `src_hedge` と `dst_hedge` | 境界、陽性 | `the_hedge_event_is_info_and_carries_no_line_text` |
| T-RP-37-014 | `たぶん明日は雨かもしれません。`（推量の語が `たぶん` と `かもしれ` の 2 つ）を zh へ。答えは `明天会下雨。` | 推量の語が複数 | 1 件 | `Ok`、log が 1 件で、原文の推量の語は `たぶん` か `かもしれ` のどちらか 1 つ（specified、RP-37 の「1 件」。log は行につき 1 件） | 境界 | `several_hedge_words_on_one_side_log_one_event` |
| T-RP-37-015 | en から zh、ko、es。`It might rain tomorrow.`、答えは推量の語が無い文（001 の 3 文） | 原文の言語が en | 止まらずに出る | `Ok`、log 1 件、原文の推量の語が `might`、訳の推量の語が null（specified、RP-37 の「原文と食い違う」。原文の言語を ja に限る文は無い） | 陽性 | `an_english_source_dropped_hedge_into_zh_ko_es_is_logged` |
| T-RP-37-016 | ja から fr。`HEDGED`、答えは `Il pleuvra demain.` | 語の表が無い言語 | 止まらずに出る | `Ok`、log 0 件。log を出す言語は zh、ko、es だけ（specified、RP-37 と 3.6 の R8i） | 境界、陰性 | `a_language_without_a_marker_table_logs_no_hedge_event` |
| T-RP-37-017 | `彼は欠席した。`（否定の語を持たない否定の意味）を zh、ko、es へ。答えは否定の語が無い文（zh `他出席了。`、ko `그는 출석했어요.`、es `Él asistió.`） | 原文に語が無い否定（10 の H36 の穴） | 止まる側へは入らない | `Ok`、log 0 件。止まらない穴を固定する。穴を塞いだ日にこの行が落ち、H36 と一緒に書き直す（specified、10 の H36） | 穴の固定 | `a_negation_with_no_word_in_the_source_is_a_known_hole` |
| T-RP-37-018 | seed 1 の組の行数：`for l in en zh ko es; do wc -l < testdata/baseline/pairs_seed1/$l.ids.txt; done` | 組の大きさ | 一致 | 85、85、68、85（specified、6 の RP-37 の行）。0 行の組を通さない | 境界 | - |
| T-RP-37-019 | seed 0 との id の重なり：言語ごとに `comm -12 <(sort testdata/baseline/pairs/$l.ids.txt) <(sort testdata/baseline/pairs_seed1/$l.ids.txt) \| wc -l` | 組が重ならない | 0 | 4 言語とも 0（specified、6 の RP-37 の行）。陽性対照：同じ式で seed 0 を seed 0 と照らすと 50（式が重なりを数えている） | 陰性、陽性 | - |
| T-RP-37-020 | seed 1 の ids の sha256 の頭 16 文字：言語ごとに `sha256sum < testdata/baseline/pairs_seed1/$l.ids.txt \| cut -c1-16` | 置いた組が記録と同じ | 一致 | en、es、zh が `2df3ae789b209ed0`、ko が `06a99159f14f7101`（derived、置いた時に `state.json` へ書いた値）。違えば組が入れ替わっている | 陽性 | - |
| T-RP-37-021 | `<main>` と `<branch>` に、seed 0 と seed 1 の 8 組（seed と言語の組）で `vprobe --pairs <dir> --lang <l>`（`--out` なし）を打つ。各出力を scratchpad の `.log` に残す | 出力が空でない | 1 組につき 4 行 | 1 組の出力に `reference`、`break:number`、`break:negation`、`break:uncertainty` の 4 行が在り、全部 `rows=` が 1 以上（`rg -c 'rows=[1-9]' <log>` が 4）。0 件の出力を「止まる数が減らない」と読まない（specified、6 の RP-37 の行） | 陽性 | - |
| T-RP-37-022 | 021 の出力の `break:number` の行：組ごとに `held=` と `ids=` を `<main>` と `<branch>` で比べる | 数の壊した行の止まる数 | `<branch>` が `<main>` 以上 | 全 8 組で `held=` が `<branch>` ≥ `<main>`、`ids=` の集合差 `<main>` − `<branch>` が空（specified、RP-37 の「数と否定の壊した行の止まる数が今の main 以上」）。陰性の build（zh で数を止めない）では差が空にならない | 陰性 | - |
| T-RP-37-023 | 021 の出力の `break:negation` の行：022 と同じ比べ方 | 否定の壊した行の止まる数 | `<branch>` が `<main>` 以上。`<main>` が R8i の前の木の時だけ例外が 1 組 | `<main>` が R8i の前の木なら、例外は zh の seed 0 の `<main>` − `<branch>` が `1512` の 1 行だけ（`held=` は 9 から 8、`rows=13`）で、残りの 7 組は差が空。`<main>` が R8i の後の木（今の main）なら全 8 組で差が空（derived、10 の H36 の免除。1512 は `testdata/baseline/pairs/zh.ids.txt` の 37 行目で、原文が `可能性があります` を持ち、R8i の前の木は推量の偶然で止めていた）。陽性対照：`rg -c '^1512$' testdata/baseline/pairs/zh.ids.txt` が 1。陰性の build（zh で否定を止めない）では 1512 以外の id も差に出る | 境界、陰性 | - |
| T-RP-37-024 | 021 の出力の `reference` の行：組ごとに比べる | 参照訳の止まる数 | `<branch>` が `<main>` 以下 | 全 8 組で `held=` が `<branch>` ≤ `<main>`、`ids=` の集合差 `<branch>` − `<main>` が空（specified、RP-37 の「参照訳の止まる数が今の main 以下」）。陰性の build（zh で推量の食い違いを止める）は zh の `held=` が増え、この行が落ちる | 陰性 | - |
| T-RP-37-025 | 021 の `break:negation` の `<branch>` の値を記録値と照らす | 記録した下限 | 以上 | `rows=` と `held=` が seed 0 は zh 13 と 8、ko 13 と 12、es 10 と 9。seed 1 は zh 20 と 18、ko 16 と 15、es 18 と 18。後の build の `held=` はこの値以上（derived、10 の H36 の「止まらない行」zh 5/13 と 2/20、ko 1/13 と 1/16、es 1/10 と 0/18 から引いた） | 境界 | - |
| T-RP-37-026 | 2 回目の打ち直し：`<main>` と `<branch>` に `vprobe --pairs testdata/baseline/pairs --lang en` と `vprobe --pairs testdata/baseline/pairs_seed1 --lang en`（`--out` なし）を打つ。出力を scratchpad の `.log` に残す | en の 2 組（seed 0 と seed 1）。ja→en の推量を止めない build を対象にする | 1 組につき 4 行 | 各出力に `reference`、`break:number`、`break:negation`、`break:uncertainty` の 4 行が在り、全部 `rows=` が 1 以上（`rg -c 'rows=[1-9]' <log>` が 4）。0 件の出力を「止まる数が減らない」と読まない（specified、6 の RP-48 の行。RP-37 の 021 の en だけの打ち直し） | 陽性 | - |
| T-RP-37-027 | 026 の `break:number` と `break:negation` の行：2 組それぞれで `<main>` と `<branch>` を比べる | 数と否定の壊した行の止まる数 | `<branch>` が `<main>` 以上 | 2 組とも、2 つの行で `held=` が `<branch>` ≥ `<main>`、`ids=` の集合差 `<main>` − `<branch>` が空（specified、RP-48 の「数と否定の壊した行の止まる数が今の main 以上」）。陰性の build（ja→en で否定と数を止めない）では差が空にならない | 陰性 | - |
| T-RP-37-028 | 026 の `break:uncertainty` の行と `reference` の行：2 組それぞれで `<main>` と `<branch>` の `held=` を記録する | 推量の壊した行と参照訳の止まる数 | 記録だけ（判定は 027 と 024 の不等号） | `break:uncertainty` の `held=` は `<branch>` ≤ `<main>` になり得る（推量の止まりを外したため）。減った数を言語の行ごとに書く。`reference` の `held=` は `<branch>` ≤ `<main>`、`ids=` の集合差 `<branch>` − `<main>` が空（derived、RP-48 の「推量の止まりが減った数を言語の行ごとに記録する」。参照訳が新しく止まる理由は無い） | 境界、陰性 | - |
| T-RP-37-029 | 026 の en の行の結果を、H36 の held-out の数と並べて読む：held-out の壊した行の止まる率は数 33/33 | 同じ build で、数の止まる率が下がっていない | 33/33 のまま | dev の en の 2 組の `break:number` が `held=rows=` のまま下がらず、held-out の数も 33/33 のまま（derived、`rust_port.ja.md` の 10 の H36 と RP-49 の「数の壊した行の止まる数は 33/33 のまま」）。held-out の打ち直しは RP-44 の席の手で、この行は dev の数だけを判定する | 境界 | - |

実装済みの行は 001 から 007 である（001 から 004 は 1 本の関数）。001 から 017 のうち、未実装の行は 008 から 017 である（実装の席が足す）。003 と 008 は ja→en の推量を止めない向きで、実装の席が期待値を書き直す。
018 から 029 は command を打つ行で、test の名前を持たない。
陰性の build と落ちる行：zh で推量の食い違いを止める build は 001、002、014、015、024 が落ちる。**en への訳で推量の食い違いを止める build**（ja→en の `uncertainty`）は 003、008 と 028 の `reference` が落ちる。**en→ja で原文の推量を落とした訳を止めない build** は 004 が落ちる（009 は訳だけの推量で、止めずに出す側なので落ちない）。**訳だけの推量も止める今の en→ja の build** は 009 が落ちる。zh で否定を止めない build は 006、010、011、023 が落ちる。zh で数を止めない build は 007、022 が落ちる。en への訳で否定と数を止めない build は RP-48 の 003、004 と 027 が落ちる。落ちる行は表から読んだ予測で、build を作って打ち直すまでは NOT VERIFIED である。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書くが、実装済みの test は `rp37_` + 末尾である。この節は実装に合わせた。
- 001 から 004 が 1 本の関数にまとまっている。落ちた時に分かるのは言語で、行（001 から 004 のどれか）ではない。行ごとに関数を分けるかは実装の席が決める。
- 止めた行が `translate.hedge.logged` を出さないこと（006、007、010、011）。設計は「止めずに出した訳」の log だけを言う。
- log の欄の key の名前。設計は「原文と訳の推量の語」までで、`src_hedge` と `dst_hedge` は実装の名前である。
- 陰性の build（zh で推量を止める、en への訳で止める、ja への訳で止めない、zh で否定と数を止めない）を作る席と作り方。6 の RP-37 の陰性欄は build を名指しするが、誰が作るかは書いていない。
- seed 1 の id の集合が「dev の ja と対象の言語の重なりの id の集合から seed 0 の id を除いた集合」と同じであることを `jq` で照らす行。dev の ja の id の一覧の置き場が設計に無いので、command を置けない（持ち主は作っていない検証の席）。
- 017 は穴を固定する行で、H36 を塞ぐ変更が入った時に書き直す。その変更の持ち主は R8h の 2 の B の席。

### 4.48 RP-38（`eval` の `translate.hedge.logged` は 1 行に 1 件）

test：`native/hayamimi-relay/tests/rp38.rs`。受入 test の名前は `rp38_eval_logs_one_hedge_event_per_line`（001）。`#[ignore]` を持たない（偽の llama-server の応答だけを使う）。
関数の名前は `rp38_` に各行の「関数の末尾」を続けた物にする。
入力：`hayamimi_relay::eval::run`（`hayamimi-relay eval` が呼ぶ関数）に、`<pairs>/<code>.ja.txt`（1 行に 1 文）を置いた一時の dir と、`<out>` の一時の dir を渡し、`MemorySink` に集まった event のうち `translate.hedge.logged` を数える。原文は `HEDGED`＝`明日は雨かもしれません。`、`PLAIN`＝`明日は雨です。`、`NEGATED_SOURCE`＝`そこにいませんでした。`（RP-37 と同じ）。偽の llama-server は、答えを要求の本文に含まれる原文で選ぶ（1 行だけの run は全部の要求に 1 つの文で答えてよい）。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp38`。
`eval` の zh、ko、es の行は T だけで訳し、E を掛けない（3.6）。V を掛ける回数は 1 行につき 1 回である。ja から en の行は T と E に V を掛け、`translate.hedge.logged` は確定の訳の V の時に 1 件だけ出す（RP-48）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-38-001 | `zh.ja.txt` に `HEDGED` の 1 行。答えは `明天会下雨。`（推量の語が無い） | 推量の食い違いが在る zh の 1 行 | 1 件 | `translate.hedge.logged` が 1 件で `lang` が `zh`。`zh.hyp.txt` の 1 行目が答えと同じ（specified、RP-38 の「同じ行の `translate.hedge.logged` が 1 件」） | 陽性 | `eval_logs_one_hedge_event_per_line` |
| T-RP-38-002 | 001 と同じ組。答えは `明天可能会下雨。`（推量の語が原文と揃う） | 食い違いが無い | 0 件 | `translate.hedge.logged` が 0 件で、`zh.hyp.txt` の 1 行目が答えと同じ（specified、RP-38 の陽性欄「推量の食い違いの無い行で 0 件」） | 陽性、log の陰性 | `eval_logs_nothing_when_the_hedge_agrees` |
| T-RP-38-003 | 001 と同じ入力で、偽の server が受けた要求の数を数える | zh は T だけ | 要求 1 件 | 要求が 1 件で、`translate.hedge.logged` が 1 件。E の要求が無いので、V は 1 回しか掛からない（specified、3.6 の「ja と en の外の言語の訳は T と V だけで、E を掛けない」） | 境界 | `a_zh_line_is_translated_by_t_alone` |
| T-RP-38-004 | zh に代えて ko（答え `내일은 비가 와요.`）、es（答え `Lloverá mañana.`）を、001 の形で 1 言語ずつ run する | 言語ごと | 1 件ずつ | 各 run の `translate.hedge.logged` が 1 件で、`lang` がその言語の code（specified、RP-38。R8i の対象は zh、ko、es） | 陽性 | `ko_and_es_log_one_event_each` |
| T-RP-38-005 | `zh.ja.txt` に 3 行：`HEDGED`（答え `明天会下雨。`）、`PLAIN`（答え `明天会下雨。`）、`PLAIN`（答え `明天可能会下雨。`） | 1 つの file に複数行。食い違う行と揃う行が混ざる | 行ごとに 0 か 1 | `translate.hedge.logged` が 2 件で、`id` が 1 行目と 3 行目の物になり、2 行目の物が無い。`zh.hyp.txt` は 3 行とも答えと同じ（specified、RP-38 の「1 行に 1 件」。`id` に eval が付ける値は設計に無く、実装は 1 始まりの行の番号） | 境界 | `only_the_mismatched_lines_log_and_each_logs_once` |
| T-RP-38-006 | `zh.ja.txt` に `NEGATED_SOURCE` の 1 行。答えは `我在那里。`（否定を落とした） | V が止める行 | 0 件 | `translate.hedge.logged` が 0 件、`translate.eval.held` が 1 件、`zh.hyp.txt` の 1 行目が空（specified、RP-37 の「否定を落とした訳は止まる」と eval の「止めた行は空の行」。log の対象は出した訳） | 陰性 | `a_held_line_logs_no_hedge_event` |
| T-RP-38-007 | `en.ja.txt` に `HEDGED` の 1 行。T と E の答えは `It will rain tomorrow.` | ja から en の組。推量が原文だけ | 1 件 | `translate.hedge.logged` が 1 件（T と E の両方に V を掛けても 1 行に 1 件）で `lang` が `en`、`translate.eval.held` が 0 件、`en.hyp.txt` の 1 行目が答えと同じ（specified、6 の RP-48 の「eval でも 1 行に 1 件」） | 陽性 | `the_ja_to_en_route_logs_a_dropped_hedge_once_and_does_not_hold` |
| T-RP-38-008 | 007 と同じ組。T と E の答えは `It might rain tomorrow.` | ja から en の組。推量が揃う | 0 件 | `translate.hedge.logged` が 0 件、`translate.eval.held` が 0 件、`en.hyp.txt` の 1 行目が答えと同じ（specified、RP-37。007 の log の陰性対照） | 陽性 | `the_ja_to_en_route_passes_an_agreeing_hedge_without_an_event` |

実装した行は 001 と 002 で、003 から 008 は未実装である（実装の席が足す）。
陰性の build（口は実装の席が置く）：(a) zh の行にも E を掛け、T の答えと E の答えのそれぞれに V を掛けて log を出す形に `eval.rs` を変えた build。001 が 2 件になり、003 が要求 2 件になって落ちる。(b) log をファイルにつき 1 件にまとめる build。005 が 1 件になって落ちる。

設計の決め：RP-38 の陰性欄は `settle_other` で V を 2 回掛けた build である（`rust_port.ja.md` の 6 の RP-38。zh に E は掛けない）。関数の名前は 4.1 の 3 つの形のうち `rp38_` + 末尾である。`eval` が `translate.hedge.logged` に付ける `id` は、その file の 1 始まりの行の番号の文字列である（`eval.rs` の `verdict` が `line.to_string()` を渡す）。

### 4.49 RP-39（zh、ko、es の倍の語を読む V の数の検査）

test：`native/hayamimi-relay/tests/rp39.rs`。受入 test の名前は `rp39_number_scales_read_in_zh_ko_es`。`#[ignore]` を持たない（`numbers.rs` の読みと偽の llama-server の応答だけを使う）。
case 行は test-designer の席が書く（`rust_port.ja.md` の 6 の RP-39 の行と 3.6 の R8l から）。T-RP-37-012 の元の 3 文（zh `预算是5百万日元。`、ko `예산은 5백만 엔이에요.`、es `El presupuesto es de 5 millones de yenes.`）をこの ID の陽性の行に入れる。

### 4.50 RP-40（字幕の頁と中継の SSE に LAN から届く）

test：`native/hayamimi/tests/rp40.rs`。受入 test の名前は `rp40_pages_and_relay_events_reach_from_the_lan`（001 から 006）。`#[ignore]` を持つ（本物の `hayamimi` と `hayamimi-relay` を起こす）。
関数の名前は `rp40_` に各行の「関数の末尾」を続けた物にする。
入力：`<hy>`＝`native/target/release/hayamimi`、`<relay>`＝`native/target/release/hayamimi-relay`（先に `cargo build --release -p hayamimi -p hayamimi-relay` で作る。無ければ test は path を名指しして落ちる）。起こす command は `<hy> --wav tests/golden/ja/ja_014.wav --serve 0 [--serve-host 0.0.0.0]` と `<relay> live --serve 0 [--serve-host 0.0.0.0]`。LAN の address は、test が UDP の socket を公開 address に向けて connect し（何も送らない）、その local address として引く IPv4 で、loopback ではない。各 server の port は、起動の stderr の URL の行から読む。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi --test rp40 -- --ignored`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-40-001 | `<hy>` を `--serve-host 0.0.0.0` で起こし、LAN の address へ `GET /` | LAN の端末から字幕の頁 | 200 | status が 200（specified、RP-40） | 陽性 | `pages_and_relay_events_reach_from_the_lan` |
| T-RP-40-002 | 001 の `<hy>` へ LAN の address から `GET /events` | LAN の端末から `hayamimi` の SSE | 200 と型 | status が 200、`content-type` が `text/event-stream` で始まる（specified、RP-40 の「`hayamimi` の `/events` も `text/event-stream`」） | 陽性 | 001 と同じ 1 本 |
| T-RP-40-003 | `<relay> live --serve 0 --serve-host 0.0.0.0` を起こし、LAN の address へ `GET /events` | LAN の端末から中継の SSE | 200 と型 | status が 200、`content-type` が `text/event-stream` で始まる（specified、RP-40） | 陽性 | 001 と同じ 1 本 |
| T-RP-40-004 | 003 の中継へ LAN の address から `GET /` | 中継の `/` は頁を持たない | 404 | status が 404（specified、RP-40 の「中継の `/` は頁を持たず 404 のまま」） | 境界、陰性 | `the_relay_has_no_page_and_answers_404` |
| T-RP-40-005 | 001 と 003 の 2 つの server へ `127.0.0.1` から同じ 3 つの GET（`<hy>` の `/` と `/events`、中継の `/events`） | loopback でも同じ | 001 から 003 と同じ | 001、002、003 と同じ status と型（specified、RP-40 の陽性対照） | 陽性 | 001 と同じ 1 本 |
| T-RP-40-006 | 001 と 003 の stderr | 起動の URL の行 | 1 行ずつ | `http://<LAN の address>:<port>` を含む行が、各 process で 1 行だけ。後から増えない（specified、RP-40 の「その address の URL が 1 行」と 8 の R5b の「LAN の URL を 1 行」） | 境界 | 001 と同じ 1 本 |
| T-RP-40-007 | `--serve-host` を付けずに `<hy>` と `<relay>` を起こす | 既定の bind | LAN は拒む | LAN の address への接続が失敗し、loopback への `GET /`（`<hy>`）と `GET /events`（中継）は通る。stderr に LAN の address の URL の行が無い（specified、RP-40 の陰性欄と 8 の R5b の「既定は `127.0.0.1`」） | 陰性 | `without_serve_host_the_lan_address_is_refused` |
| T-RP-40-008 | `--serve-host 127.0.0.1` を明示して `<hy>` と `<relay>` を起こす | 既定と同じ値の明示 | 007 と同じ | 007 と同じ（LAN は拒まれ、LAN の URL の行が無い）（derived、8 の R5b の「既定は `127.0.0.1`」） | 境界 | `an_explicit_loopback_serve_host_is_the_default` |

実装した行は 001 から 003、005 から 007 で、004 と 008 は未実装である（実装の席が足す）。
陰性の build（`--serve-host` を付けない run で LAN に繋がる形）で落ちる行は 007 と 008 である。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書くが、実装の test は `rp40_` + 末尾である。この節は実装に合わせた。
- `--serve-host` に address でない値や、持たない address（IPv6、機械に無い IPv4）を渡した時の振る舞い。設計に条件が無いので行にしない。
- 起動の URL の行の書式。001 から 003 と 006 は `http://<address>:<port>` を含む行で数える。
- `<relay>` の build を test が起こすか、先に作るか。この節は先に作る形にした。

### 4.51 RP-41（`stream` の `listen` が `/events` を SSE で読む）

test：`native/hayamimi-eval/tests/rp41.rs`。受入 test の名前は `rp41_stream_listen_reads_events_over_sse`（001）。偽の SSE の server の行は `#[ignore]` を持たず、本物の `/events` の行（007）だけが持つ。
関数の名前は `rp41_` に各行の「関数の末尾」を続けた物にする。
入力：偽の server は 127.0.0.1 の空き port で 1 接続だけ受け、`GET /events HTTP/1.1` の行を控える。本文の枠は 2 通りで、chunked（`hayamimi` の `/events` の形）と、接続が閉じるまで続く形（中継の形）。`final` は `{"type":"final","id":"mic-1","text":"hello"}`、`translation` は `{"type":"translation","id":"mic-1","text":"hallo"}`。`listen(host, port, origin)` が返す物に `finish(drain)` を呼んで、受けた event（時刻 µs と本文）を読む。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp41`。007 は `<hy> --serve 0` を起こしてから同じ command に `-- --ignored` を足す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-41-001 | 偽の SSE の server が `text/event-stream` で `final` と `translation` を 1 件ずつ送る。2 通りの枠で | 正常な SSE | 2 件 | 受けた event が 2 件で、型が `final`、`translation` の順。時刻は 1 件目 ≤ 2 件目 ≤ 起点からの経過。server が受けた要求の行が `GET /events HTTP/1.1`（specified、RP-41 の「`listen` が 2 件の event を受け、時刻を記録する」） | 陽性 | `stream_listen_reads_events_over_sse` |
| T-RP-41-002 | 001 の server に、受けた本文を照らす：`final` の `text` が `hello`、`translation` の `text` が `hallo` | 本文が欠けない | 一致 | 記録した本文の JSON が送った JSON と同じ（specified、RP-41。型だけでなく中身を記録する。001 は型しか見ない） | 陽性 | `the_recorded_events_carry_the_bodies_that_were_sent` |
| T-RP-41-003 | 偽の server が `text/html` で答える | 型が SSE でない | `Err` | `Protocol` の error（derived、RP-41：`listen` は SSE の答えだけを読む） | 陰性 | `listen_refuses_an_answer_that_is_not_an_event_stream` |
| T-RP-41-004 | 偽の server が `404 Not Found` で答える | 状態が 200 でない | `Err` | `Http` の error（derived、003 と同じ） | 陰性 | 003 と同じ 1 本 |
| T-RP-41-005 | 閉じている port へ `listen` | 繋がらない | `Err` | `Connect` の error（implicit） | 境界、陰性 | `listen_to_a_closed_port_is_a_connect_error` |
| T-RP-41-006 | 偽の server が `session_start` を送ってから `final` を送る。`start()` を呼ぶ | 起動の合図 | 待つ | `start()` が `session_start` を受けてから返り、受けた event は `session_start`、`final` の順（derived、8 の R8m：`listen` が Rust の `/events` を読む形） | 陽性 | `start_waits_for_the_kept_session_start` |
| T-RP-41-007 | 偽の server が `session_start` を送らずに `final` だけ送って閉じる。`start()` を呼ぶ | 合図が来ない | `Err` | `Protocol` の error で、ready の timeout まで待たない（20 秒未満）（derived） | 境界、陰性 | `start_fails_when_the_stream_ends_without_session_start` |
| T-RP-41-008 | 偽の server が 50 ms 後に閉じる run と、500 ms 開けておく run | 閉じた接続の報告 | 区別 | 閉じた run は `closed_by_server` が真、開けた run は偽で event が 1 件（derived） | 境界 | `a_stream_the_server_closed_is_reported` |
| T-RP-41-009 | `<hy> --serve 0` の本物の `/events` に `listen` する。`<hy>` は `--wav` を再生する | 本物の server | 1 件以上 | 受けた event が 1 件以上で、各本文が JSON の `type` を持つ（specified、RP-41 の陽性欄の `--ignored` の行） | 陽性 | `listen_hears_the_real_events_of_hayamimi` |

実装済みの行は 001、003 から 008（003 と 004 は 1 本の関数）、未実装の行は 002 と 009 である（実装の席が足す）。
陰性の build（WebSocket で繋ぐ今の `listen`）で落ちる行は 001、006、007、008 である。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書くが、実装の test は `rp41_` + 末尾である。この節は実装に合わせた。
- 006 から 008 の `start` と `closed_by_server` は、設計の RP-41 の文に無い（実装が足した API）。`listen` を SSE にする変更の周りで守る物として、この席が行に起こした。
- SSE の注釈行（`:` で始まる行）、`event:` の欄、複数行の `data:` の読み方。設計に条件が無いので行にしない。
- 009 の本物の `<hy>` の起こし方と音声（`--wav` の指定）。

### 4.52 RP-42（両方止めた行の理由が log で読める）

test：`native/hayamimi-relay/tests/rp42.rs`。受入 test の名前は `rp42_held_line_logs_both_reasons_without_the_text`（001）。`#[ignore]` を持たない（偽の llama-server だけを使う）。
関数の名前は `rp42_` に各行の「関数の末尾」を続けた物にする。
入力：偽の llama-server は 1 本目の要求（T）に第 1 の文、2 本目以降（E）に第 2 の文を答える。原文 `SOURCE`＝`五人はそこにいませんでした。`（数 5 と否定の語 `ません`）、T の文 `Five were there.`（数を残し否定を落とす）、E の文 `They were not there.`（否定を残し数を落とす）。`PLAIN_SOURCE`＝`そこに五人います。`、その訳 `Five people are there.`。ja→en の 1 行を `Pipeline::run` に流し、`Translation` と、集まった log の event を読む。dev 用の file は `VerifyDump` が開く一時の path である。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp42`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-42-001 | `SOURCE`、T は `Five were there.`、E は `They were not there.`。`VerifyDump` を名指しした path で開く | T と E の両方が V に止められる | 止める | `status` が `held`、`text` が空、`hold_reason`（E の側）が `number`。`translate.line.held` が 1 件で、`hold_reason` が `number`、T の側の理由が `negation`。どの event の JSON にも T の文と E の文が入らない。dev 用の file は 2 行で、1 行目が T の文、2 行目が E の文（specified、RP-42。実装の key は `t_hold_reason` と、行の `id`、`pass`、`text`） | 陽性 | `held_line_logs_both_reasons_without_the_text` |
| T-RP-42-002 | `PLAIN_SOURCE`、T は `Five people are there.`、E は同じ文に `</s>` を付けた文 | T は V を通り、E は止まる | T の理由を載せない | `status` が `held`、`hold_reason` が `form`、`translate.line.held` に T の側の理由の欄が無い（specified、RP-42 の陽性欄「T が V を通る行では T の側の理由を載せない」） | 陰性 | `a_line_whose_t_passed_logs_no_reason_for_t` |
| T-RP-42-003 | `SOURCE`、T も E も `Five were there.` | 2 つの理由が同じ | 両方載る | `hold_reason` と T の側の理由がどちらも `negation`（specified、RP-42。同じ値でも欄を省かない） | 境界 | `both_reasons_are_logged_when_they_agree` |
| T-RP-42-004 | `observer` を渡さない run と、`VerifyDump::open` に開けない path（dir）を渡した run | file は名指しされた時だけ | 作らない | 名指ししない run は一時の dir に何も作らない。開けない path は `translate.dump.failed`（error_code `HYM_STORAGE_DUMP_OPEN`）が 1 件で `None`（specified、RP-42 の「起こす側が path を渡した時だけ」。欄の名前は実装の物） | 境界、陰性 | `the_dump_exists_only_when_a_file_is_named_and_opens` |
| T-RP-42-005 | 中継（`relay::start_with`）の `RelayConfig` に dev 用の file を持たせ、偽の認識器が `SOURCE` の `final` を 1 件送る | 中継の lane が行を渡す | 2 行 | dev 用の file の 2 行が T の文と E の文で、id が `final` の `id`。中継の log に T の文と E の文が入らない。`translate.line.held` が 1 件（specified、RP-42。観測は実際の経路で出る） | 陽性 | `the_relay_writes_the_rows_of_the_lines_it_translates` |
| T-RP-42-006 | 001 の形で T の文だけを入れ替える。`SOURCE` に対し `Six were not there.`（数）、`Sure, five were not there.`（前置き）。原文 `五人はAcmeにいませんでした。` に対し `Five were not there.`（Latin 語の名前を落とす）。E は常に `They were not there.` を答える。別に `Maybe five were not there.`（推量）を同じ形で流す | T の理由の取り得る値 | 3 通りと、推量の 1 件 | T の側の理由が順に `number`、`form`、`proper_noun` と一致する。推量の訳は ja→en では止まらず（RP-48）、`status` が `ok` で `translate.line.held` が 0 件（specified、RP-42 の「V の理由と同じ enum の値」と RP-48。`negation` は 001 が持ち、`uncertainty` は ja→en の行の理由に取れない） | 境界 | `every_reason_that_holds_a_ja_to_en_line_can_be_the_reason_of_t` |
| T-RP-42-007 | `PLAIN_SOURCE`、T も E も `Five people are there.`（E まで通る） | 止まらない行 | 載せない | `status` が `ok`、`translate.line.held` が 0 件。dev 用の file を開いていても止まらない行の文は log に出ない（specified、RP-42 は「両方止めた行」の観測） | 陰性 | `a_line_that_goes_out_logs_no_held_event` |
| T-RP-42-008 | 中継の `RelayConfig` の dev 用の file を持たせない（`None`）run。001 と同じ行を流す | `--verify-text` を渡さない | 作らない | 一時の dir にも他の場所にも file が作られず、`translate.line.held` は T の側の理由つきで 1 件（specified、RP-42 の「起こす側が path を渡した時だけ」、10 の H56） | 陰性 | `a_relay_without_a_named_file_writes_none` |
| T-RP-42-009 | `hayamimi-relay live` の引数の解釈：`--verify-text` を付けない引数と、`--verify-text <path>` を付けた引数 | 起こす側が path を渡した時だけ（引数から `RelayConfig` まで） | 付けた時だけ Some | 付けない引数は dev 用の file の指定が無く、付けた引数は `<path>` を持つ（specified、8 の R8o の「`--verify-text <file>` を付けた時だけ」、10 の H56。008 は `RelayConfig` から先、これは引数から `RelayConfig` まで） | 境界 | `live_without_verify_text_names_no_file` |

| T-RP-42-010 | `SOURCE`、T は `Five were there.`（否定を落とす）、E は `Five were not there.`（V を通る） | T が V に止められ、E が通る | 1 件 | `status` が `ok`、`text` が E の文。`translate.draft.held`（info）がちょうど 1 件で、`id`、`source`（`file`）、`hold_reason`（`negation`）を持つ。どの event の JSON にも T の文と E の文が入らない（specified、D 3.18 の 5 の log） | 陽性 | `a_line_whose_t_was_held_and_e_passed_logs_the_draft_held` |
| T-RP-42-011 | `PLAIN_SOURCE`、T も E も `Five people are there.` | T が V を通る | 0 件 | `status` が `ok`、`translate.draft.held` が 0 件（specified、同上） | 陰性（対照） | `a_line_whose_t_passed_logs_no_draft_held` |
| T-RP-42-012 | 001 と同じ（T も E も V に止められる） | 両方止まる | 0 件 | `status` が `held`、`translate.draft.held` が 0 件、`translate.line.held` が 1 件（specified、同上。この行の T の理由は `t_hold_reason` が持つ） | 陰性 | `a_line_held_on_both_passes_logs_no_draft_held` |

実装済みの行は 001 から 005 と 010 から 012 で、006 から 009 は未実装である（実装の席が足す）。009 は `cli.rs` の引数の解釈（`verify_text: Option<PathBuf>`）を読む。
陰性の build（T の側の理由を載せない build）で落ちる行は 001、003、006 である。log に止めた訳の本文を載せる build は 001、005 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書くが、実装の test は `rp42_` + 末尾である。この節は実装に合わせた。
- 007 は、設計が言わない物（止まらない行が log を出さないこと）をこの席が置いた境界である。
- 止める理由の値のうち `unavailable`（llama-server が答えない）は、T の側の理由として載るか。T が答えない行には V が掛からず、設計に条件が無いので行にしない。
- 006 の `proper_noun` は、原文の Latin 語（`Acme`）が訳に残らない時の理由である。この理由が出る検査は `verify.rs` の Latin 語の検査で、設計に条件の文が無い。

### 4.53 RP-43（否定の表の「なさそう」「なさげ」「なさすぎ」）

test：`native/hayamimi-relay/tests/rp43.rs`。受入 test の名前は `rp43_nasasou_is_read_as_negation`（001 から 003）。表の行は `#[ignore]` を持たない。E2E-1 の打ち直しの 70/80（009）は、`e2e1_mic_process_ja_to_en_beats_python`（4.63）が実走の記録で確かめる。
関数の名前は `rp43_` に各行の「関数の末尾」を続けた物にする。
入力：`Verifier::open(None, sink)` が持つ内蔵の表に、日本語の原文と訳を渡して `verify(src, translation, Lang::Ja, sink)` の判定（`Ok` か `Held(理由)`）を読む。llama-server は使わない。表は `native/hayamimi-relay/data/verify_markers.toml` の `ja.negation` と `exclude`。
command（001 から 008）：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp43`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-43-001 | 否定を残した訳：`あまり美味しくなさそうです。`→`It doesn't seem very tasty.`、`彼は興味がなさげでした。`→`He was not interested.`、`やる気がなさすぎます。`→`He has no motivation at all.` | 「なさそう」「なさげ」「なさすぎ」の 3 語 | `Ok` | 3 件とも `Ok`（specified、RP-43 の「否定を残した en の訳は止まらない」） | 陽性 | `nasasou_is_read_as_negation` |
| T-RP-43-002 | 001 の原文で、否定を落とした訳：`It seems very tasty.`、`He was interested.`、`He has too much motivation.` | 同じ 3 語 | 止まる | 3 件とも `Held(Negation)`（specified、RP-43 の「否定を落とした訳は `negation` で止まる」） | 陰性 | 001 と同じ 1 本 |
| T-RP-43-003 | 否定の語でない 4 文：`早く食べなさい。`→`Eat quickly.`、`ごめんなさい。`→`I'm sorry.`、`先生がお話しなさる。`→`The teacher speaks.`、`皆さん、静かにしなさい。`→`Everyone, be quiet.` | 「なさ」を含む否定でない語 | `Ok` | 4 件とも `Ok`（specified、RP-43 の「なさい」「なさる」「ごめんなさい」は否定の語と読まない） | 境界、陽性 | 001 と同じ 1 本 |
| T-RP-43-004 | `それは美味しくない。`→`It is not tasty.`（残した訳）と `It is tasty.`（落とした訳） | 今の表の「ない」の行 | 前と同じ | 残した訳は `Ok`、落とした訳は `Held(Negation)`（specified、RP-43 の陽性対照） | 陽性 | `the_nai_row_reads_as_before` |
| T-RP-43-005 | 否定でない語の 11 文：`つまらなさそうな顔をしている。`→`He seems bored.`、`少なさげな数字です。`→`The number looks small.`、`危なさそうな道です。`→`The road seems dangerous.`、`申し訳なさそうな顔をしている。`→`He seems apologetic.`、`仕方なさそうに笑った。`→`He seems resigned and smiled.`、`情けなさそうな声だった。`→`His voice seemed pitiful.`、`頼りなさそうな人です。`→`He seems unreliable.`、`申し訳ない気持ちです。`→`I feel sorry.`、`仕方ない話です。`→`It is unavoidable.`、`情けない結果です。`→`The result is pitiful.`、`頼りない人です。`→`He is unreliable.` | 語幹が「な」の形容詞と、否定の意味を持たない定型の語（10 の H56）。「〜なさそう」と基本形 | `Ok` | 11 件とも `Ok`。否定を持たない正しい訳を止めない（specified、10 の H56 の持ち主の決め） | 境界、陽性 | `a_positive_adjective_with_the_stem_na_is_not_negation` |
| T-RP-43-006 | 活用した形：`ご飯が美味しくなさそうに見える。`→`It does not seem tasty.`（残す。推量の語 `seem` と否定の語 `not` を持つ）と `The food looks tasty.`（落とす）、`興味がなさげな顔だった。`→`He was not interested.`（残す）と `He looked interested.`（落とす）、`やる気がなさすぎて困る。`→`He has no motivation, which is a problem.`（残す）と `He has motivation, which is a problem.`（落とす） | 3 語の後ろに続く字 | 語で読む | 残す 3 件は `Ok`、落とす 3 件は `Held(Negation)`（derived、RP-43：表の語は部分文字列で読む。1 件目の残す訳が `seem` を持つのは 10 の H59 の機構による） | 境界 | `the_conjugated_forms_are_read_as_negation_too` |
| T-RP-43-007 | `それは本物ではなさそうです。`→`It does not seem to be genuine.`（残す）と `It seems to be genuine.`（落とす） | 「では」を伴う形 | 語で読む | 残す訳は `Ok`、落とす訳は `Held(Negation)`（derived、RP-43。「なさそう」の前の字に依らない） | 境界 | `a_nasasou_after_dewa_is_read_as_negation` |
| T-RP-43-008 | 001 の原文で、訳が否定を足した文：`あまり美味しそうです。`→`It does not seem tasty.` | 原文に否定の語が無く訳に有る（逆向き） | 止まる | `Held(Negation)`（derived、否定の検査は有無の一致を見る） | 陰性 | `a_negation_added_to_a_plain_source_is_held` |
| T-RP-43-009 | E2E-1 の打ち直し：llama-server、中継、process A を起こして `<ev> stream --wav testdata/lat-ja.wav --repeat 5 --port 8766 --out e2e1.jsonl` を打ち、`cargo test --release --test e2e1 -- --ignored` を打つ | 実走 | 実走 | 訳の取れた標本が 70/80 以上（70/80＝0.875 は 20/23＝0.8696 以上、69/80＝0.8625 は未満。70 は通り 69 は落ちる）。utt 16 の 4 件のうち 3 件以上が戻る。71/80 は見込みで、確かめていない（NOT VERIFIED）（specified、RP-43） | 境界 | - |
| T-RP-43-010 | 否定を残した原文 2 つ：`申し訳なさそうではない。`→`He does not seem apologetic.`、`仕方なさそうにしない。`→`He does not appear resigned.` | 「〜なさそう」に否定が続く形で、訳が推量の語（`seem`、`appear`、`look like`）を持つ | `Ok` | 2 件とも `Ok`。原文の `そう` が推量の表に当たり、訳の推量の語と揃う（derived、10 の H59 の機構：`exclude` は否定の表にだけ効き、推量の表の `そう` は `さそう` に当たる） | 穴の固定 | `a_negated_nasasou_with_a_hedge_word_in_the_translation_passes` |
| T-RP-43-011 | 010 の原文で、訳が推量の語を持たない文：`He is not apologetic.`、`He does not look resigned.`（`look` だけで `look like` ではない） | 同じ原文で、訳に推量の語が無い | 止まらない | 2 件とも `Ok`、`translate.hedge.logged` が 1 件。原文だけが推量を持つ食い違いは ja→en では止めず log にだけ残す（specified、RP-48。否定を残した正しい訳は出る。以前の `Held(Uncertainty)` の固定は RP-48 で外れた） | 境界 | `a_negated_nasasou_without_a_hedge_word_in_the_translation_goes_out_and_logs_the_hedge` |
| T-RP-43-012 | 006 の原文で、残す訳の別の書き方 2 つ：`ご飯が美味しくなさそうに見える。`→`The food does not look tasty.`、`興味がなさげな顔だった。`→`He looked uninterested.` | 訳が否定を持つのに V の表が読まない形（1 件目は推量の語を持たない。2 件目は否定の接頭辞 `un-`） | 1 件目は出る、2 件目は止まる | 1 件目は `Ok`、`translate.hedge.logged` が 1 件（原文の `そう` が推量で訳に `seem`、`appear`、`look like` が無い食い違いは、ja→en では止めず log にだけ残す。RP-48）。2 件目は `Held(Negation)`（英語側の否定の語に `uninterested` が無く、原文の `なさげ` の否定に訳の否定が揃わない）。2 件目は今の挙動の固定で、否定を残した正しい訳が止まる。直すかは設計の問いで、直す日にこの行が落ち、書き直す（derived、実装の席が `rp43.rs` の草稿で打った結果） | 境界、穴の固定 | `a_conjugated_form_whose_translation_the_tables_do_not_read_goes_out_or_is_held_for_now` |

実装済みの行は 001 から 005 で（001 から 003 は 1 本の関数）、006 から 008 と 010 から 012 は未実装である（実装の席が足す）。009 は E2E の席が打つ。010 と 011 は今の `verify.rs` で通る。


陰性の build：「なさ」だけを表に足した build は 003 が落ちる（「ごめんなさい」が `negation` で止まる）。表を足さない build は 001、002、006、007 が落ちる。`exclude` を外した build は 005 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書くが、実装の test は `rp43_` + 末尾である。この節は実装に合わせた。
- 006 から 008 は、設計の文（RP-43 の期待値）から導いた境界である。「語で読む」は表が部分文字列で読む前提（`verify_markers.toml` の `ja.negation` の読み方）に依る。
- 005 の 8 件目から 11 件目（基本形）は 10 の H56 の決めである。基本形が否定でないと決めたのは `exclude` に `申し訳な` などの部分文字列を足したためで、同じ部分文字列を含む別の語（例：`申し訳ないが断る`）も否定でなくなる。この境界の行は設計が決めるまで置かない。
- 012 の 2 件目：英語側の否定の語の表は接頭辞の否定（`uninterested`、`unable` 以外の `un-`、`dis-`、`in-`）を読まないので、否定を残した正しい訳（`He looked uninterested.`）が `Held(Negation)` で止まる。H59（推量の表の `そう`）とは別の形で、機構は英語側の `en.negation` の語の表にある。表に `un-` の語を足すか、止まりを受けるかは設計が決める（この席は新しい H 番号を振らない）。
- 009 の E2E-1 の実走の command は 6 と E2E の表のとおりである。70/80 を判定する test は、実走の記録を読む受入 test `e2e1_mic_process_ja_to_en_beats_python`（`native/hayamimi-eval/tests/e2e1.rs`。4.63 の T-E2E-1-001）で、割合の境界の行は 4.63 の T-E2E-1-015（70 は通り 69 は落ちる）である。

### 4.54 RP-44（R8o の後の新しい held-out の組）

test：`native/hayamimi-eval/tests/rp44.rs`。受入 test の名前は `rp44_new_heldout_set_is_fresh_and_has_nasasou`（001 から 003）。`#[ignore]` を持ち、repo の外の組の ids と日本語の原文を読む。組を作るのは test-designer の席で、V を直す席に path を渡さない。この節に組の置き場と中身を書かない（置き場は `state.json` の RP-44 の項に受付が書く）。
関数の名前は `rp44_` に各行の「関数の末尾」を続けた物にする。
組の作り方：FLEURS の train の表（dev と test の表と id が重ならない）から、ja と対象の言語の 4 言語（en、zh、ko、es）に共通の id を、`sha256("rp44:" + id)` の昇順で 30 件取り、同じ id と同じ原文を 4 言語で使う。残る 5 行は、「なさそう」「なさげ」「なさすぎ」を含む手書きの行（id は 9000001 から 9000005。3 行が否定の語、2 行が H56 の否定でない語）で、FLEURS の表には「なさそう」を含む行が無い。言語ごとに `<lang>.ids.txt`、`<lang>.ja.txt`、`<lang>.ref.txt` を 35 行ずつ置く。手書きの 5 行の参照訳は、この席が書いた。
入力：組の ids（4 つ）と ja の原文（4 つ）、dev の組（`testdata/baseline/pairs/` と `testdata/baseline/pairs_seed1/`）の ids、test split の組の ids（id だけを読み、原文と参照訳は開かない）。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp44 -- --ignored`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-44-001 | 言語ごとの組の ids と、dev の seed 0 と seed 1 の ids の和 | 重なり | 0 | 4 言語とも重なり 0（specified、RP-44 の「dev と test split の id との重なりが 0」。独立の式：`comm -12 <(sort -u <組>) <(sort -u <dev の seed 0> <dev の seed 1>) \| wc -l`）。陽性対照：同じ式で組を組と照らすと 35 | 陽性 | `new_heldout_set_is_fresh_and_has_nasasou` |
| T-RP-44-002 | 言語ごとの組の ids と、test split の組の ids | 重なり | 0 | 4 言語とも重なり 0（specified、RP-44）。陽性対照：test split の ids を test split の ids と照らすと 50 | 陽性 | 001 と同じ 1 本 |
| T-RP-44-003 | 言語ごとの組の ja の原文 | 「なさそう」を含む行 | 1 行以上 | 4 言語とも `なさそう` を含む行が 1 行以上（実測は 3 行）。「なさげ」か「なさすぎ」を含む行が 2 行（specified、RP-44。独立の式：`rg -c 'なさそう' <組の ja>`）。陽性対照：dev の組の ja に同じ式を打つと 0 件で、組が dev と別であることを示す | 陽性 | 001 と同じ 1 本 |
| T-RP-44-004 | 組の ids の 1 行を、dev の seed 0 の id に替えた ids（fixture） | dev の id が 1 つ混ざる | `Err` | 重なり 1 を名指しした `Err`（specified、RP-44 の陰性欄「dev の id を 1 つ混ぜた ids で落ちる」） | 境界、陰性 | `a_dev_id_in_the_set_is_refused` |
| T-RP-44-005 | dev の seed 0 の組の ids を組として渡す | 重なり 50 の全部 | `Err` | 重なり 50 を名指しした `Err`（derived、RP-44 の陽性欄「dev の seed 0 の組の ids から重なりを数えられる」） | 陽性 | `a_set_equal_to_dev_seed_0_overlaps_in_every_id` |
| T-RP-44-006 | 言語ごとに `ids`、`ja`、`ref` の行の数と、空行の数 | 3 つの file が揃う | 35 と 0 | 4 言語 × 3 file とも 35 行で、空行が 0（derived、`eval --final` が行ごとに対を読む）。ids は 4 言語で同じ | 境界 | `the_three_files_of_each_language_line_up` |
| T-RP-44-007 | 言語ごとの ids の sha256 の頭 16 桁（`sha256sum < <lang>.ids.txt \| cut -c1-16`） | 記録と一致 | 一致 | `state.json` に書いた値と一致する（derived、組を作った時に測った値）。値が違えば組が差し替わっている。4 言語の ids は同じ内容なので、4 つが同じ値になる | 陽性 | `the_ids_digest_matches_the_recorded_one` |

実装済みの行は無い。`rp44.rs` は未作成で、001 から 007 を実装の席が足す。
陰性の build（ids に dev の id を混ぜる）で落ちる行は 001 と 004 である。
組を使えるのは 1 回だけである（会話字幕の CS-30 が同じ `heldout_sha256` の 2 回目を拒む）。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- FLEURS の train の表の文を、評価する翻訳 model が学習で見ていないことは確かめていない（NOT VERIFIED）。見ていれば chrF が高く出る。確かめ方は、model の学習データの公開資料を調べること（持ち主は設計の席）。
- 手書きの 5 行は参照訳が 1 本だけで、FLEURS の参照訳と性質が違う（書いた人が 1 人）。chrF の平均を FLEURS の行だけで出すか、5 行を含めるかは設計が決める。
- 組の大きさ（言語ごとに 35 行）。RP-35 の前の main の結果は 40 行だった（F35）ので、`heldout_sha256` を持つ別の batch として比べることになる。
- 001 から 003 を 1 本の関数にまとめた。落ちた時に分かるのは言語までで、行（001 から 003 のどれか）ではない。
- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書く。この節は他の節（4.46 から）と同じ `rp44_` + 末尾にした。

### 4.55 RP-45（E の雛形が字幕に出ない）

test：`native/hayamimi-relay/tests/rp45.rs`。受入 test の名前は `rp45_edit_template_never_reaches_the_subtitle`。偽の llama-server の行は `#[ignore]` を持たない。E2E-2 の打ち直しで札の語を数える行だけが記録を読む。
case 行は `rust_port.ja.md` の 6 の RP-45 の行と 8 の R8p から書いた。010 から 017 は、札の読みを行頭に狭めた後に残る形を、陽性の行と穴の固定の行として置いた（10 の H60）。

command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp45`。記録を読む行は `-- --ignored --nocapture` を足す。関数の名前は `rp45_` に「関数の末尾」を続けた物にする。
入力：偽の llama-server が 1 本目の要求（T）に `draft`、2 本目以降（E）に `edited` を答える。`Pipeline::run` が settle した `Translation` の `status`、`stage`、`text` と、`translate.edit.held` の `hold_reason` を読む。原文は en→ja が `He went inside and examined his letters, but there was nothing from Dana.`、ja→en が `彼は中に入って手紙を確認した。`。札の語は `Draf`、`Rewrit`、`English:`、`Japanese:`、`Japaese` で、V とは別に数える。
札の読み方（`edit_answer.rs` の `translation_of`）：行の頭の `Draft` 系（`Draf` で始まる 7 byte 以内）と `Rewritten` に、訳の言語名（prompt が渡した名前から 2 文字以内の綴りゆれ）と `:`（全角も）が続く形が行頭に在る時だけ札とし、最後の札の後ろを訳とする。行の途中のキーワード、言語名でない語を挟む形（`Draft law:`）、単独の `English:` は札にしない。`English:` は、答えに `Draft` か `Rewritten` の札が在る時に、最後の札の前として落ちる。`</s>` を含む答えは切らずに V へ渡す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-45-001 | en→ja の E の答え 6 種：札が前と間と後ろに在る、行末に 2 空白と `Drafted`、`Draf Japaese:` と `Rewritten Japaese:`、`Rewritten` の札だけ、同じ行に札と訳、全角の `：` と空行 | 記録の実物の形（札が前、間、後ろ。改行入り） | 6 種のすべて | `status` が `ok`、`stage` が `e`、`text` が `edited` と一致し、札の語を含まない（specified、RP-45 の「札の後ろの訳だけが確定の訳」） | 陽性 | `edit_template_never_reaches_the_subtitle` |
| T-RP-45-002 | ja→en の E の答え 2 種：`Japanese:`、`Draft English:`、`Rewritten English:` の 3 つ組と、行末に 2 空白の 3 つ組 | 逆向きの札 | 2 種 | `stage` が `e`、`text` が `edited`（specified、「ja→en の E にも同じ規則」） | 陽性 | `the_labels_are_stripped_into_english_too` |
| T-RP-45-003 | 札の無い答え（前後に空白と改行） | 札が無い | 1 件 | `text` が trim しただけの `edited`、`translate.edit.held` が 0 件（specified、RP-45 の「札の無い E の答えをそのまま確定にする」） | 陽性（対照） | `an_answer_without_labels_is_unchanged` |
| T-RP-45-004 | 最後の札の後ろが原文の言語（英語の 1 文） | 札を剥がしても訳でない | 1 件 | V が `form` で止め、`stage` が `t`、`text` が T の訳、`translate.edit.held` が `form` で 1 件（specified、V の形の検査は変えない） | 陰性 | `the_wrong_language_behind_the_label_is_withheld_for_form` |
| T-RP-45-005 | 最後の札の後ろが空。T の訳が V を通る組と、通らない組 | 空になる答え | 2 件 | 前者は `stage` が `t` で `form` が 1 件。後者は `status` が `held`、`hold_reason` が `form`、`text` が空（specified、「空になる答えを `form` で止める」） | 陰性 | `an_answer_empty_behind_its_labels_is_withheld_for_form` |
| T-RP-45-006 | 札に似た正しい訳 10 種：`The draft: nobody read it.`、`Final draft: ready.`、`The Draft Board: it called him up.`、`He said the rewritten: version is better.`、`Draft law: passed.`、`Draft plan: submit by Friday.`、`Japanese: language class`、`English: 英語の授業`、`Draft 3: the plan`、`理由は次の通り: 雨だった。` | 行の途中のキーワード、言語名でない語、単独の原文の札 | 10 種 | 両方向で `translation_of` が入力と同じ（specified、正しい訳を札と読んで落とさない）。V を通す経路は 010 が持つ | 陽性（対照） | `text_that_only_looks_like_a_label_is_not_cut` |
| T-RP-45-007 | 正しい訳 `彼はダナと\nNASAの計画について話した。`（ラテン文字の略語と改行） | V を厳しくしない | 1 件 | `stage` が `e`、`text` が `edited`、`form` が 0 件（specified、8 の R8p の「英字や改行を含む文を落とす形に厳しくはしない」） | 陽性（対照） | `latin_letters_and_a_line_break_in_a_correct_translation_still_pass` |
| T-RP-45-008 | `English: 英語の授業`、と `English:\nHi\n\nRewritten Japanese:\n英語の授業` | 単独の原文の札と、札と並ぶ原文の札 | 2 件 | 前者は入力と同じ、後者は `英語の授業`（specified、原文の札は他の札が在る時だけ落ちる） | 陽性（対照） | `a_lone_source_label_is_text` |
| T-RP-45-009 | `docs/results/r8h_verify/e2e2c.events.jsonl` のうち `type` が `translation`、`of` が `final`、`stage` が `e` の 35 件の `text` | 記録の実物 | 35 件 | 札の語を含む件が 20 件から 0 件になり、剥がして空になる件が 0 件（`jq` の独立の式で数えた 20 件と一致）。`#[ignore]` を持つ | 陽性 | `recorded_edit_answers_lose_their_labels` |

| T-RP-45-010 | ja→en の E の答え 5 種：`The draft: nobody read it.`、`Final draft: ready.`、`Draft law: passed.`、`Draft plan: submit by Friday.`、`Japanese: language class`。en→ja の E の答え 1 種：`English: 英語の授業`。T の答えは通る訳 | 札に似た形を V の経路に通す。006 は関数単体、これは `Pipeline::run` | 6 種 | 6 種とも `status` が `ok`、`stage` が `e`、`text` が答えと同じ、`translate.edit.held` が 0 件（specified、10 の H60 と 6 の RP-45：行の途中の `draft:` で答え全体が空になる形、`Draft <言語名でない語>:`、単独の原文の札を落とさない） | 陽性（対照） | `text_that_looks_like_a_label_goes_out_as_the_translation` |
| T-RP-45-011 | en→ja の E の答え：`Draft Jpanse:\n下書き\n\nRewritten Jpanse:\n` + `edited`（言語名 `Japanese` から 2 文字違う）と、同じ形で `Jpans`（3 文字違う） | 綴りゆれの境界（2 文字と 3 文字） | 2 文字は札 | 前者は `text` が `edited`（札として落ちる）。後者は札と読まず、`text` が答えの全文で、札の語（`Draft`、`Rewritten`）が残る（specified、8 の R8p の「綴りゆれは 2 文字まで」。2 文字は Levenshtein の編集距離で、`Japanese` から `Jpanse` は削除 2 回、`Jpans` は 3 回） | 境界 | `a_language_name_two_edits_away_is_a_label_and_three_is_not` |
| T-RP-45-012 | en→ja の E の答え `**Rewritten Japanese:** ` + `edited`（太字の札） | 行頭が記号で、札と読まない形 | 札の語が残る | `status` が `ok`、`stage` が `e`、`text` が答えの全文と同じで、`Rewritten` と `Japanese:` を含む。穴の固定である。行頭に無い札の形は剥がさず訳の本文として V に渡す（specified、6 の RP-45 と 10 の H60）。剥がす実装に変わった日にこの行が落ち、H60 と一緒に書き直す | 穴の固定 | `a_bold_label_is_kept_as_text` |
| T-RP-45-013 | en→ja の E の答え `Rewritten Japanese (final): ` + `edited`（言語名と `:` の間に注） | 注を挟む札 | 札の語が残る | 012 と同じ期待値（specified、RP-45、H60） | 穴の固定 | `a_label_with_a_note_before_the_colon_is_kept_as_text` |
| T-RP-45-014 | en→ja の E の答え `1. Rewritten Japanese: ` + `edited`（番号つきの札） | 行頭が番号 | 札の語が残る | 012 と同じ期待値（specified、RP-45、H60） | 穴の固定 | `a_numbered_label_is_kept_as_text` |
| T-RP-45-015 | en→ja の E の答えが 1 行に全部続く形：`English: ` + 原文 + ` Draft Japanese: ` + `draft` + ` Rewritten Japanese: ` + `edited`（改行なし） | 札が行の途中に並ぶ | 札の語が残る | 012 と同じ期待値。`text` は答えの全文（specified、RP-45、H60） | 穴の固定 | `a_template_written_on_one_line_is_kept_as_text` |
| T-RP-45-016 | en→ja の E の答え `Draft Javanese:\n` + `edited`（`Javanese` は `Japanese` と 1 文字違う別の言語名） | 行頭の札が、2 文字までの綴りゆれの内側に入る実在の別の語 | 札と読む | `text` が `edited`（`Draft Javanese:` が札として落ちる）。穴の固定である。訳の本文が `Draft Javanese:` で始まる場合も同じに落ち、本文の頭が失われる（specified、8 の R8p の「この代償は受ける」と 10 の H60） | 穴の固定 | `a_line_head_javanese_label_is_read_as_a_misspelling` |
| T-RP-45-017 | ja→en の E の答え `Draft Enlist:\n` + `edited`（`Enlist` は `English` から 2 文字違う実在の語） | 016 と同じ。目的言語が英語の側 | 札と読む | `text` が `edited`。穴の固定である（specified、R8p と H60 の `Enlist`。`English` から `Enlist` は g の削除と h から t への置換の 2 回） | 穴の固定 | `a_line_head_enlist_label_is_read_as_a_misspelling` |

実装した行は 001 から 009 で、010 から 017 は未実装である（実装の席が足す）。012 から 017 は今の実装の挙動を固定する行で、足した test は今の `edit_answer.rs` で通る。
陰性の build：(a) `pipeline.rs` の `edit` で `translation_of` を外し、`trim` だけに戻した build は 001、002、004、005 の 4 件が落ちる。(b) `names_language` を、空でない語なら真にする build は 006、010、011 が落ちる。(c) 札を行の途中でも読む build（行頭に限る前の形）は 006、010 が落ちる。
残る穴：行頭が `Draft <言語名>:` や `Rewritten <言語名>:` で始まる訳の行は札と読まれ、その前が落ちる（016 が固定する）。行の途中に残った札は剥がさず、V が他の文と同じに判定する。言語名が 2 語以上の札は読まない。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 012 から 015 の形（太字、注つき、番号つき、1 行に続く雛形）と、016、017 の綴りゆれの代償は、設計が受ける側に決めた（10 の H60、6 の RP-45）。6 行は穴の固定の行で、設計が札の読みを広げた日に落ちる。その時は H60 と一緒に書き直す。
- E2E-2 の打ち直しで札の語を含む訳が 1 件でも出たら、012 から 015 のどの形かを名指しして設計へ戻す（6 の RP-45）。この判定は記録を読む 009 の同じ式で数え、行頭か否かを問わない。
- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書くが、実装の test は `rp45_` + 末尾である。この節は実装に合わせた。
- キーワードの `Draft` 系の幅（`Draf` で始まる 7 byte 以内）の境界の行。設計に幅の文が無いので置かない。

### 4.56 RP-46（英語の認識器を最初の英語の発話より前に読む）

test：`native/hayamimi/tests/rp46.rs`。受入 test の名前は `rp46_english_recognizer_loads_before_ready`（001 から 003）。001、002、004 は `#[ignore]` を持つ（本物の認識器を読む）。ほかの行は fixture か偽の hub で通常の run に入る。
関数の名前は `rp46_` に各行の「関数の末尾」を続けた物にする。ただし 006 から 014 の関数は、実装の席が `rp46_` を前置きにしていない。そこは全名を書いた。
入力：`<hy> --input ws --lang en --ws-port 0`（004 は `--lang ja`）を起こし、`/ingest` へ `testdata/eval_real/en_01.wav`（英語 1 発話）を、発話の後ろに 2 秒の無音を置いて 2 回送る。`ws-port 0` は client に port を知らせないので、test は少し前まで空いていた port を指定する。順序は、program が stderr に出す行（`asr.model.loaded`、`ingest.client.connected`）の並びで読む。`ingest.client.connected` は program が `ready` を client に送った直後に出る行で、`ready` の代わりに使う。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi --test rp46 -- --ignored`。ほかの行は同じ command から `-- --ignored` を除く。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-46-001 | `--lang en` の run で、最初の発話を送る前の stderr の行の並び | 英語の認識器を読む時 | `ready` より前 | `asr.model.loaded` で `model` が `v3` の行が、`ingest.client.connected` の行より前に在る。v3 の読み込みが 1 回だけ（specified、RP-46 の「`asr.model.loaded`（`model` が `v3`）が `/ingest` の `ready` より前に出る」） | 陽性 | `english_recognizer_loads_before_ready` |
| T-RP-46-002 | 001 の run の 1 発話目と 2 発話目の `final` の `decode_ms` と、v3 の `load_ms` | 最初の発話に読み込みを含めない | 1 発話目 < `load_ms`、1 発話目 ≤ 2 発話目の 10 倍 | 1 発話目の `decode_ms` が v3 の `load_ms` より小さく、2 発話目の `decode_ms` の 10 倍以下（specified、RP-46。「同じ桁」を 10 倍以下とこの席が置いた。「`load_ms` を足した値より小さく」は、2 発話目の `decode_ms` に `load_ms` を足した値と読むと 1 発話目に読み込みを含む run が通りうるので、`load_ms` そのものより小さいと読んだ） | 境界 | 001 と同じ 1 本 |
| T-RP-46-003 | 001 の run で client が 2 発話を送る | 比べる発話が揃う | 2 件以上 | `final` が 2 件以上出る。1 件しか出ない run は 002 を判定せず、件数を名指しして落ちる（implicit。空の入力を通過と読まない） | 境界、陰性 | 001 と同じ 1 本 |
| T-RP-46-004 | `--lang ja` の run の stderr の行の並び | 日本語の session | ReazonSpeech は `ready` より前 | `asr.model.loaded` で `model` が `rz` の行が `ingest.client.connected` より前に在る。`v3` の読み込みが 0 回（specified、RP-46 の陽性欄「`--lang ja` の run で ReazonSpeech の読み込みを `ready` より前に見る」。`v3` が 0 回は、英語の認識器を日本語の session で読まないという derived の境界） | 陽性 | `japanese_session_loads_only_the_japanese_recognizer_before_ready` |
| T-RP-46-005 | 振り分けで en を使う設定の run（`--lang` を付けず、LID が en に振る設定） | 8 の R8q の「振り分けで en を使う設定」 | `ready` より前 | 001 と同じ並び（derived、8 の R8q）。設定の書き方が設計に無く、行は書けるが command を置けない | 陽性 | 設計の席が設定を決めるまで未実装 |
| T-RP-46-006 | 偽の stderr の行：`rz` の読み込み、`v3` の読み込み、`ingest.client.connected` の順 | 順序の判定関数の正例 | 通る | 判定が `Ok`（specified、RP-46） | 陽性 | `a_load_before_the_client_was_answered_passes` |
| T-RP-46-007 | 偽の行：`rz`、`ingest.client.connected`、発話の行、`v3` の読み込みの順 | 読み込みが `ready` の後（RP-46 が止める形） | 落ちる | 判定が `Err`（`after the client` を含む）（specified、RP-46 の陰性欄） | 陰性 | `a_load_after_the_client_was_answered_is_refused` |
| T-RP-46-008 | 偽の行：`ingest.client.connected` だけ、または `v3` の読み込みだけ | 行が欠ける | 落ちる | どちらも `Err`。読み込みの行が 0 件の run を「ready より前」と読まない（implicit） | 境界、陰性 | `a_model_that_was_never_loaded_or_a_client_never_answered_is_refused` |
| T-RP-46-009 | 偽の行 `rz` が 900 ms、`v3` が 8000 ms で、`v3` と `pz` の `load_ms` を引く | 読み込みの時間は名指しした model の物 | 引ける | `v3` は 8000、`pz` は無く `Err`（derived） | 境界 | `the_load_time_is_read_from_the_model_it_names` |
| T-RP-46-010 | 1 発話目 900 ms、2 発話目 700 ms、`load_ms` 8000 ms | 読み込みを含まない 1 発話目 | 通る | `Ok`（specified、RP-46） | 陽性 | `a_first_decode_without_the_load_is_of_the_order_of_the_second` |
| T-RP-46-011 | 1 発話目 8401 ms、2 発話目 420 ms、`load_ms` 7979.6 ms（読み込みを含む 1 発話目）と、1 発話目が `load_ms` と等しい組（8000、8000、8000） | `load_ms` の境界（超える側と等号） | 落ちる | どちらも `Err`（`holds the model load` を含む）（specified、RP-46 の「読み込みの時間を含まない」。等号は含むと読む） | 境界、陰性 | `a_first_decode_that_holds_the_load_is_refused` |
| T-RP-46-012 | 1 発話目 4000 ms、2 発話目 300 ms、`load_ms` 8000 ms と、1 発話目 3000 ms と 3000.1 ms（2 発話目 300 ms） | 「同じ桁」の境界（10 倍） | 10 倍以下 | 4000 は `Err`（`order` を含む）、3000 は `Ok`、3000.1 は `Err`（この席が置いた 10 倍の両側） | 境界、陰性 | `a_first_decode_far_slower_than_the_second_is_refused` |
| T-RP-46-013 | 偽の hub に client が繋ぎ config を送る。セッションがまだ始まっていない | `ready` はセッション開始の後 | 600 ms 内に来ない | 600 ms の間 `ready` が来ない（derived、RP-46 の並びの仕組み：`ready` は認識器の読み込み後のセッション開始で返す）。`session_start` を出した後 5 秒内に `ready` が来る | 境界、陰性 | `ready_waits_for_the_session_to_start` |
| T-RP-46-014 | 013 の hub が先に `session_start` を出している | セッションが始まった後 | すぐ来る | `ready` が 5 秒内に来る（derived） | 陽性 | `ready_is_answered_at_once_after_the_session_started` |

実装した行は 001 から 004 と 006 から 014 である（005 は未実装で、設定の書き方が要る）。
陰性の build（最初の英語の発話で読み込む今の形）で落ちる行は 001、002 である。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 「`load_ms` を足した値より小さく」の読み方。002 は `load_ms` そのものより小さいと読んだ（上の欄）。2 発話目の `decode_ms` との和と読む設計なら、002 の閾値が変わる。
- `ready` を client が受けた時刻を直接測る手段が無い（`/ingest` の `ready` を受ける client の時刻を 001 は取らず、`ingest.client.connected` の行で代用した）。この代用が `ready` の後に出ることは program の実装に依る。
- 005 の「振り分けで en を使う設定」。
- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書く。この節は 001 から 005 を実装の `rp46_` + 末尾に合わせ、006 から 014 は実装の名前のまま全名の末尾（`rp46_` を前置きにしない）を書いた。

### 4.57 RP-47（新しい確定文の T を前の行の E の後ろに並べない）

test：`native/hayamimi-relay/tests/rp47.rs`。受入 test の名前は `rp47_next_draft_is_not_queued_behind_the_previous_edit`（001）。`#[ignore]` を持たない（偽の llama-server だけを使う）。
関数の名前は `rp47_` に各行の「関数の末尾」を続けた物にする。
入力：偽の llama-server は要求ごとに別の thread で答え、T の要求は即答し、E の要求は 1000 ms 遅れて答える（言語の code を持つ要求は 300 ms）。各要求の到着の時刻と答えた時刻を控え、同時に出ていた要求の最大数（peak）を数える。中継は本物の HTTP でこの server に繋がり、`Lanes::PerSource` で走る。偽の認識器が `final` を送る。2 つ目の確定文は 1 つ目の 200 ms 後に送る。slot の数は 4（3.6 の 5。`LLAMA_SLOTS`）。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp47`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-47-001 | mic から確定文を 2 つ（200 ms 間隔）。E は 1000 ms | 前の行の E が出ている間に次の確定文が来る | 2 つ目の T が 1 つ目の E の答えより前 | 1 つ目の E の到着 < 2 つ目の T の到着（E が出ている間に来た）。2 つ目の T の到着 + 400 ms ≤ 1 つ目の E の答え（余裕 400 ms は、E の 1000 ms と間隔 200 ms から、T が 200 ms 台、E の答えが 1000 ms 以降の差 800 ms に収まる値としてこの席が置いた）。2 つ目の下書きの表示が 1 つ目の確定の表示より前。peak は 2 から 4（specified、RP-47 の「2 つ目の T の依頼が 1 つ目の E の応答より前に出る」） | 陽性 | `next_draft_is_not_queued_behind_the_previous_edit` |
| T-RP-47-002 | 001 の入力 | 行ごとに 1 つの訳 | 1 つずつ、順に | どの行も 1 回だけ settle し、訳が自分の原文から作られ、1 つ目の settle が 2 つ目の settle より先に配信される（specified、RP-47 は順序を変えない。訳の配信は到着の順） | 境界 | `every_line_is_settled_once_and_in_the_order_it_came` |
| T-RP-47-003 | mic、loopback、file の 3 音源から、それぞれ確定文を 2 つ（100 ms 間隔）。E は 1000 ms | 3 音源が同時に次の確定文を出す。E が 3 つ出ている間に T が 3 つ来る（要求 6 本を並べようとする） | peak ≤ 4 | peak が 4 に等しい（上限に達し、超えない）。6 件とも settle する（specified、RP-47 の「slot の数を超えて依頼を並べない」。peak が 4 に達することは、上限が効いた証拠で、上限を外すと 6 になる） | 境界、陰性 | `no_more_requests_are_made_at_once_than_the_server_has_slots` |
| T-RP-47-004 | zh だけを訳す中継（E が無い）。mic から確定文を 2 つ（100 ms 間隔） | E の無い言語 | T が直列 | 要求が 2 件で全部「言語の code を持つ要求」。1 つ目の答え ≤ 2 つ目の到着（直列）。2 件とも `status` が `ok`、`lang` が `zh`（specified、RP-47 の陽性欄） | 陽性 | `a_language_without_an_edit_asks_its_translations_one_after_the_other` |
| T-RP-47-005 | 001 と同じ入力で、2 つ目の確定文を 1500 ms 後に送る（1 つ目の E の答えの後） | 間隔が E より長い | 順序の判定は変わらない | 2 つ目の T の到着 ≥ 1 つ目の E の答え。peak が 1（derived、RP-47：並べるのは E が出ている間だけ。E の無い時間に並べる物は無い） | 境界 | `a_second_line_after_the_edit_asks_alone` |
| T-RP-47-006 | 001 の入力を 3 行に増やす（100 ms 間隔、E は 1000 ms） | 同じ音源の lane に 3 行が重なる | peak ≤ 4 | peak が 4 以下で、3 行とも settle し、配信が到着の順（derived、RP-47：slot 4 の範囲で、T を E より先に頼む） | 境界 | `three_lines_of_one_source_stay_within_the_slots_and_in_order` |

実装した行は 001 から 004 で、005 と 006 は未実装である（実装の席が足す）。
陰性の build（訳の lane が 1 本で直列の今の形）で落ちる行は 001（2 つ目の T の到着が 1 つ目の E の答えの後）と 006 である。slot の上限を外す build は 003 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 関数の名前の規則。4.1 は「受入 test の名前 + `_` + 末尾」と書くが、実装の test は `rp47_` + 末尾である。この節は実装に合わせた。
- 001 の余裕 400 ms は、偽の server の待ちの値に依る。実機の E は 1000 ms と限らず、設計は余裕の値を言わない。
- 003 の peak が 4 に等しいことは、偽の server が 6 本を同時に受けられる前提の値で、設計は「超えない」までを言う。
- E が答えない（接続が切れる、timeout）行で、並べた次の T がどう動くか。設計に条件が無いので行にしない。

### 4.58 RP-48（ja→en の推量の不一致を止めず log にだけ残す）

test：`native/hayamimi-relay/tests/rp48.rs`。受入 test の名前は `rp48_hedge_mismatch_into_english_is_logged_and_into_japanese_held`（001）。`#[ignore]` を持たない（偽の llama-server の応答だけを使う）。
関数の名前は `rp48_` に各行の「関数の末尾」を続けた物にする。
入力：原文と偽の server の使い方は 4.47 と同じ（`HEDGED`＝`明日は雨かもしれません。`、`PLAIN`＝`明日は雨です。`、`NEGATED_SOURCE`＝`そこにいませんでした。`）。ja から en は T と E の組で、server は T の依頼と E の依頼を別々に数え、それぞれに決めた答えを返す。en から ja は原文が英語で、答えが日本語である。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp48`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-48-001 | 4 つの向き：(a) ja から en、`HEDGED`、T と E の答えは `It will rain tomorrow.`。(b) ja から en、`PLAIN`、答えは `It might rain tomorrow.`。(c) en から ja、`It might rain tomorrow.`、答えは `明日は雨が降ります。`。(d) en から ja、`It will rain tomorrow.`、答えは `明日は雨かもしれません。` | 推量の食い違いが原文だけ／訳だけ × 訳の向きが en／ja | (a)(b) 止まらない、(c) 止まる、(d) 止まらない | (a) `Ok`、`translate.hedge.logged` が 1 件、`lang` が `en`、原文の推量の語が `かもしれ`、訳の推量の語が null。(b) `Ok`、1 件、原文の語が null、訳の語が `might`。(c) `Held`、理由 `uncertainty`、log 0 件。(d) `Ok`、log 1 件（specified、6 の RP-48 の ja→en と、RP-63 の en→ja。(a)(b)(c)(d) は T-RP-37-003、008、004、009 と同じ入力と期待値） | 陽性（a、b、d）、陰性（c） | `hedge_mismatch_into_english_is_logged_and_into_japanese_held` |
| T-RP-48-002 | (a) の入力で、server が受けた依頼の数と、`translate.hedge.logged` の数を数える | T と E の両方が V を通る | 依頼 2 件、log 1 件 | 依頼が 2 件（T と E）、log が 1 件（確定の訳の V の時だけ。T の V と E の V の両方で出すと 2 件になる）（specified、6 の RP-48 の「確定の訳の V の時だけ出す」） | 境界、陰性 | `the_pair_route_logs_once_for_the_settled_translation` |
| T-RP-48-003 | `NEGATED_SOURCE` を ja から en へ。T と E の答えは `I was there.`（否定を落とした） | ja→en でも否定は止める | 止まる | `Held`、理由 `negation`、log 0 件（specified、RP-48 の「否定を落とした訳は `negation` で止まる」）。陽性対照：答えを `I was not there.` にすると `Ok` | 陰性、陽性 | `a_lost_negation_into_english_is_still_held` |
| T-RP-48-004 | `予算は五百万円です。` を ja から en へ。T と E の答えは `The budget is seven million yen.` | ja→en でも数は止める | 止まる | `Held`、理由 `number`、log 0 件（specified、RP-48 の「数を変えた訳は数の理由で止まる」）。陽性対照：答えを `The budget is five million yen.` にすると `Ok` | 陰性、陽性 | `a_changed_number_into_english_is_still_held` |
| T-RP-48-005 | `そこにいないかもしれません。`（否定と推量が同居）を ja から en へ。答えは `He is there.`（否定も推量も無い） | 否定の食い違いと推量の食い違いが同居 | 止まる | `Held`、理由 `negation`、log 0 件。止めた行は推量を log に残さない（specified、RP-48。4.47 の T-RP-37-010 の en 版。理由の順は `verify_with` が決め、NOT VERIFIED） | 境界、陰性 | `a_held_line_into_english_logs_no_hedge_event` |
| T-RP-48-006 | ja から en。(a) `HEDGED` の答え `It might rain tomorrow.`、(b) `PLAIN` の答え `It will rain tomorrow.` | 推量が揃う 2 通り | 止まらずに出る | 2 つとも `Ok`、log 0 件（specified、RP-48 の陽性欄「推量の食い違いの無い ja→en の訳を止めずに出し、log が 0 件」） | 陽性、log の陰性 | `agreeing_hedges_into_english_log_nothing` |
| T-RP-48-007 | `たぶん明日は雨かもしれません。`（推量の語が 2 つ）を ja から en へ。答えは `It will rain tomorrow.` | 推量の語が複数 | 1 件 | `Ok`、log が 1 件で、原文の推量の語は `たぶん` か `かもしれ` のどちらか 1 つ（specified、RP-37 の「1 件」と同じ形） | 境界 | `several_hedge_words_into_english_log_one_event` |
| T-RP-48-008 | (a) の入力の log 1 件の形 | log の欄 | 欄の集合 | `level` が info、欄は `id`、`lang`、`src_hedge`、`dst_hedge` の 4 つ。`message` と欄の値に原文の文と訳の文が入らない（specified、3.6 の R8i と `observability-and-log-monitoring.md`。T-RP-37-013 と同じ式） | 境界、陽性 | `the_english_hedge_event_carries_only_the_four_fields` |
| T-RP-48-009 | `eval` の ja→en 1 行（`HEDGED`、T と E の答えは `It will rain tomorrow.`） | `eval` でも 1 行に 1 件 | 1 件 | T-RP-38-007 と同じ（`translate.hedge.logged` 1 件、`translate.eval.held` 0 件、`en.hyp.txt` の 1 行目が答えと同じ） | 陽性 | T-RP-38-007 の関数 |
| T-RP-48-010 | dev の seed 0 と seed 1 の en の組で `vprobe --lang en` を `<main>` と `<branch>` に打つ | 数と否定の壊した行、推量の記録 | 数と否定が `<main>` 以上 | T-RP-37-026 から 029（command は 4.47 に在る）が 4.47 の 2 回目の打ち直しの行である。この行は重複して持たず、そちらを RP-48 の merge の前の条件とする（specified、6 の RP-48） | 陰性 | - |

陰性の build と落ちる行（表から読んだ予測で、NOT VERIFIED）：ja→en で推量を止める今の build は 001 の (a)(b)、002、007、008、009 と T-RP-37-003、008 が落ちる。en→ja で原文の推量を落とした訳を止めない build は 001 の (c) と T-RP-37-004 が落ちる。ja→en で否定と数を止めない build は 003、004、005 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 6 の RP-48 の期待値欄は「en→ja の同じ食い違い（訳だけの推量）は `uncertainty` で止まる」と書き、8 の R8t と RP-63 は「止まらずに出て log 1 件」と書く。001 の (d) は RP-63 に合わせた。RP-48 の欄と受入 test の名前（`..._and_into_japanese_held`）が古い（持ち主は設計の席）。
- 001 は 4 つの向きを 1 本の関数にまとめる受入の形で、落ちた時に分かるのは向き（a から d）で、行ではない。行ごとに関数を分けるかは実装の席が決める。
- T が V を通り、E が V に落ちる行で、log が T の訳の 1 件になるか 0 件になるかは、設計が「確定の訳」までしか言わず、E が落ちた時に確定する訳が T かは書いていない。行にしない（持ち主は R8s の 1 の実装の席）。
- 005 の理由の順（`negation` が `uncertainty` より先か）は `verify_with` の実装に依る。設計は両方の食い違いが在る時の理由を言わない。
- `live` の経路（`pipeline.rs`）が `settle_pair` と同じ log の回数を守るかは、設計に受入が無く、行にしていない。
- 009 は T-RP-38-007 を指す。名前は 4.48 の `the_ja_to_en_route_logs_a_dropped_hedge_once_and_does_not_hold` で、実装の席が 4.48 の側へ置く。

### 4.59 RP-49（語で書いた半時の 30 を読む）

test：`native/hayamimi-relay/tests/rp49.rs`。受入 test の名前は `rp49_spelled_clock_time_is_read_as_hour_and_minute`。`#[ignore]` を持たない。
関数の名前は `rp49_` に各行の「関数の末尾」を続けた物にする。
入力：行は原文と訳の組を V に渡す（`Verifier::verify`。ja→en は原文が ja、en→ja は原文が en）。読みの規則は D の 6 の RP-49：ja の `N時半` と en の `half past N` に 30 を 1 つ足して読む。`one thirty` は今のとおり 1 と 30 と読み、時と分の組には読まない（`one` は V が比べる数の下限 2 に届かず、30 だけが残る）。`N時30分`、`N時35分` は数字がそのまま 30、35 の値を持つ。zh、ko、es への訳には 30 を足さない。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp49`。
半時の 30 を読まない build で落ちるのは、`半` の側に 30 が無く `one thirty` の側だけが 30 を持つ行である。独立検証が打った実測では 001 だけが落ち、002、004、006 は通る。003、005、008 の `Held` は 30 の有無に依らず止まり、007 も通る行である（表から読んだ予測、NOT VERIFIED）。通る行は読みの拡張が他の読みを動かしていない対照で、半時の読みの有無は区別しない。同じ理由で 010 は落ちる行に数える（予測、NOT VERIFIED）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-49-001 | `1時半に会いましょう。` の訳が `Let's meet at one thirty.`。同じ原文で `At one thirty, let's meet.`（ja→en） | 半時の語。大文字の `At` を含む | 止まらずに出る | 2 つとも `Ok`（specified、6 の RP-49 の「`Let's meet at one thirty.` は ja→en でも止まらずに出る」。`1時半` が 30 を 1 つ持ち、`one thirty` が 1 と 30 のうち 30 だけを持つので揃う） | 陽性。半時の 30 を読まない今の build で `Held`／`number` になり落ちる（実装の前に今の main で再現する） | `half_past_one_matches_at_one_thirty` |
| T-RP-49-002 | `1時30分に会いましょう。` の訳が `Let's meet at one thirty.` | 「30分」の書き方 | 止まらずに出る | `Ok`（specified、6 の RP-49 の「`1時30分`→`at one thirty`」。30 を読む前から通る） | 陽性。拡張が数字の読みを動かさない対照 | `one_thirty_in_digits_matches_at_one_thirty` |
| T-RP-49-003 | `1時半に会いましょう。` の訳が `Let's meet at two thirty.` | 時が違う | 止まる | `Held`、理由 `number`（specified、6 の RP-49 の「`two thirty` を `1時半` の訳にした行は数の理由で止まる」）。001 が対 | 陰性 | `two_thirty_for_half_past_one_is_held` |
| T-RP-49-004 | `1時35分に着きます。` の訳が `I arrive at one thirty-five.` | 分が 2 語の英語（`thirty-five`）と数字の `35` | 止まらずに出る | `Ok`（derived、6 の RP-49 の「`1時30分`→`at one thirty`」と同じ読み。`one` は下限に届かず、35 が両側に残る）。半時の 30 を読まなくても通る | 陽性。拡張が他の読みを動かさない対照 | `one_thirty_five_matches_thirty_five_past_one` |
| T-RP-49-005 | `1時35分に着きます。` の訳が (a) `I arrive at one thirty-six.`、(b) `I arrive at one thirty.` | 分が違う | 止まる | 2 つとも `Held`、理由 `number`（derived、時と分の組として読む規則。004 が対） | 陰性 | `a_different_minute_is_held` |
| T-RP-49-006 | `12時10分に会います。` の訳が `I meet at twelve ten.`。`12時59分` の訳が `I meet at twelve fifty-nine.` | 時が 12 で分が 10 と 59 | 止まらずに出る | 2 つとも `Ok`（derived、時と分の数字が両側に同じ値で残る。半時の 30 を読まなくても通る） | 境界、対照 | `the_ends_of_the_hour_and_minute_ranges_are_read_as_a_clock_time` |
| T-RP-49-007 | 時刻でない読み：`30人が来ました。` の訳が `Thirty people came.`、`31日です。` の訳が `It is the thirty-first.`、`500万円です。` の訳が `It is 5 million yen.` | 時の語の後でない `thirty`。序数。倍の語 | 止まらずに出る | 3 つとも `Ok`。`numbers.rs` の読みは `thirty-first` が 31、`5 million` が 5000000 のまま（specified、6 の RP-49 の陽性欄。今の main で通る行で、時刻の読みを足しても動かない対照） | 陽性、境界 | `readings_outside_a_clock_time_stay_as_they_were` |
| T-RP-49-008 | `1時に会いましょう。` の訳が (a) `Let's meet at one.`、(b) `Let's meet at two.` | 分が続かない時の語 | (a) 止まらない、(b) 止まる | (a) `Ok`、(b) `Held`、理由 `number`（specified、6 の RP-49 の「半時の語の外の数の読みは今のまま」。(a) は `one` が下限に届かず、(b) は 2 と 1 が食い違う） | 境界、陰性 | `an_hour_word_without_a_minute_word_is_read_as_before` |
| T-RP-49-009 | dev の seed 0 と seed 1 の en の組で `vprobe --lang en` を `<main>` と `<branch>` に打つ | 数の壊した行の止まる数 | 33/33 のまま | T-RP-37-026 から 029 が 2 回目の打ち直しの行である。`break:number` の `held=` が `rows=` と同じ値のまま下がらない（specified、6 の RP-49 の「数の壊した行の止まる数は 33/33 のまま」） | 陰性 | - |
| T-RP-49-010 | en の原文 `At one thirty he went to Rectors for lunch, and when he returned a messenger was waiting for him.` の訳が (a) `1時半になると、彼はレクターズで昼食を取りに行き、戻ると使者が待っていた。`、(b) 同じ文の `2時半` | en→ja。E2E の行 | (a) 止まらない、(b) 止まる | (a) `Ok`、(b) `Held`、理由 `number`（specified、6 の RP-49 の「ja→en でも en→ja でも止まらずに出る」と「`two thirty` を `1時半` の訳にした行は数で止まる」） | 陽性、陰性。(a) は半時の 30 を読まない build で落ちる | `the_e2e_line_goes_out_in_both_directions` |
| T-RP-49-011 | `At half past one he went out.` と `1時半に彼は出かけた。` を ja→en と en→ja の両方に | 英語の半時の語 | 止まらずに出る | 2 つとも `Ok`（specified、6 の RP-49 の「`half past one`→`1時半` も止まらない」。`half past one` は 30 を 1 つ足して読む） | 陽性、両方向 | `half_past_one_still_matches_half_past_one` |
| T-RP-49-012 | 値の読み：`compared_values("At one thirty", Tongue::English)` と `compared_values("1時半に行った。", Tongue::Other)` | 読みの値。`半時の語の 30` を足す前の読み | 英語は 30 だけ、日本語は空 | 英語は `[30 * MICRO]`、日本語は空（derived、6 の RP-49 の「`one thirty` は今のとおり 1 と 30 と読む」。1 は下限に届かない）。足した後の読みの値は設計が言わず、この行は足す前の値の固定である | 境界 | `the_plain_reading_has_the_thirty_on_one_side_only` |
| T-RP-49-013 | `1時半に昼食を取りに行った。` を zh へ訳した `一点半去吃午饭了。`（`verify_into`） | zh への訳 | 30 を足さない | `Ok`（specified、6 の RP-49 の「zh、ko、es への訳には 30 を足さない」）。zh の側に 30 が無くても止まらない | 境界、陽性 | `a_translation_into_zh_is_not_asked_for_the_thirty` |

陰性の build と落ちる行：半時の 30 を読まない今の build は 001 が `Held`／`number` で落ちる（実測）。010 も落ちる（予測、NOT VERIFIED）。単位の語の次に十の位の語を一般に続けて読む build（`one thirty` を 31 と読む）は、001 の `1時半` の行が数の理由で止まって落ちる（`1時半` の側は 30、`one thirty` の側は 31）。zh、ko、es にも 30 を足す build は 013 が落ちる（予測）。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 時の語が `thirteen`以上、分が 10 未満（`one oh five`、`one five`）、分が 60 以上の読み。設計は `one thirty` を時と分の組に読まない形に決めたので、これらも今の読みのままで、行を置かない。
- 012 の `compared_values` は、`numbers.rs` の公開の関数と `MICRO` の単位に依る。足した後の `half past one` の値は設計に無い。
- 009 の 33/33 は H36 の held-out の数で、dev の en の `rows=` ではない。dev の en の行の数と内訳は設計に無く、009 は「下がらない」までを判定する。
- 002、004、005 の (b)、006 は半時の読みを区別しない。区別する行は 001 と 010 だけである。

### 4.60 RP-63（en→ja の推量は原文の推量が落ちた時だけ止め、義務の形を否定と読まない）

test：`native/hayamimi-relay/tests/rp63.rs`。受入 test の名前は `rp63_into_japanese_a_dropped_hedge_is_held_and_an_added_hedge_is_logged`（001 から 003）。`#[ignore]` を持たない（偽の llama-server の応答だけを使う）。
関数の名前は `rp63_` に各行の「関数の末尾」を続けた物にする。実装の branch（`feat/r8t`）は `tests/r8t_hedge.rs` と `tests/r8t_obligation.rs` に `r8t_` の名前で置いている。case 行の名前と置き場は D に従い、実装側の名前を直すのは実装の席である。
入力：原文は en（推量）と ja（義務）。V に原文と訳を渡し、`Verdict` と `translate.hedge.logged` の件数を見る。推量の表は `verify_markers.toml`（en の推量の表に `could` は無い）。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp63`。vprobe の行（008）は cargo の test ではなく、検証の席が打つ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-63-001 | en から ja。(a) `I might go.`→`行きます。`、(b) `He might look for it at once.`→`彼はすぐにそれを探す。`、(c) `I think it is late.`→`もう遅いです。` | 原文が推量を持ち、訳が持たない | 止まる | 3 つとも `Held`、理由 `uncertainty`、`translate.hedge.logged` 0 件（specified、6 の RP-63 の「原文が推量を持ち訳が持たない行は `uncertainty` で止まる」。(b)(c) は実装の席の例で、推量の語が `might` と `think`） | 陰性 | `into_japanese_a_dropped_hedge_is_held_and_an_added_hedge_is_logged` |
| T-RP-63-002 | en から ja。(a) `It will rain.`→`雨が降るでしょう。`、(b) `I could go.`→`行けるかもしれません。`、(c) `He would look for it at once.`→`彼はすぐにそれを探すだろう。` | 原文が推量を持たず、訳だけが持つ。`could` と `would` は en の表に無い | 止まらずに出る | 3 つとも `Ok`、`translate.hedge.logged` が 1 件（specified、6 の RP-63。(c) は実装の席の例） | 陽性。訳だけの推量も止める今の build で落ちる | 001 と同じ 1 本 |
| T-RP-63-003 | en から ja。`I might go.`→`行くかもしれません。`。`It will rain.`→`雨が降ります。` | 推量が揃う 2 通り | 止まらずに出る | 2 つとも `Ok`、log 0 件（specified、6 の RP-63 の「両方が推量を持つ行は今のとおり出る」） | 陽性、log の陰性 | 001 と同じ 1 本 |
| T-RP-63-004 | 義務の過去形と「なくては」の形の 8 つの原文を、否定を持たない英語の訳へ：`宿題をしなければならなかった。`、`宿題をしないといけなかった。`、`宿題をしなくてはならない。`、`宿題をしなくてはなりません。`、`宿題をしなくてはならなかった。`、`宿題をしなくてはいけない。`、`宿題をしなくてはいけません。`、`宿題をしなくてはいけなかった。`。訳は過去形の 3 つが `I had to do my homework.`、他は `I must do my homework.` | `exclude` に足した 8 つの形。ja から en | 止まらない | 8 つとも `Ok`（specified、6 の RP-63 の「8 つの形の原文の、否定を持たない正しい訳は止まらない」。8 つの形は D の 6 の RP-63 の列挙と同じ） | 陽性。8 つを足さない今の build で過去形の 2 つ（`なければならなかった`、`ないといけなかった`）と `なくては〜` の 6 つが `negation` で止まり落ちる（予測、NOT VERIFIED） | `a_past_obligation_is_not_a_negation` |
| T-RP-63-005 | 004 の 8 つの原文を、義務を否定にした訳へ：`I did not have to do my homework.` | 義務の形の原文で、訳だけが否定 | 止まる | 8 つとも `Held`、理由 `negation`（specified、6 の RP-63 の「同じ原文の義務を落とした否定の訳は `negation` で止まる」） | 陰性 | `an_obligation_source_with_a_negated_translation_is_held` |
| T-RP-63-006 | `宿題をしなかった。`→`I did my homework.`（ja から en）。en から ja では `He did not come.`→`彼は来た。` | 本当の否定。ja の側と en の側 | 止まる | 2 つとも `Held`、理由 `negation`（specified、6 の RP-63 の「本当の否定は今のとおり止まる」。2 つ目は実装の席の例） | 陰性 | `a_real_negation_is_still_held` |
| T-RP-63-007 | (a) `雨にならなかった。`→`It rained.`、(b) `それは美味しくない。`→`It is not delicious.`、(c) `それは美味しくない。`→`It is delicious.` | `ならなかった` を単独で除かない。8 つの外の今の否定の行 | (a)(c) 止まる、(b) 止まらない | (a)(c) `Held`、理由 `negation`、(b) `Ok`（specified、6 の RP-63 の陰性欄と陽性欄の「8 つを含まない今の否定の行を今のとおり読む」） | 陰性（a、c）、陽性（b）。`ならなかった` を単独で除く build で (a) が止まらず落ちる | `a_short_past_of_naru_is_not_hidden_from_the_negation_table` |
| T-RP-63-008 | dev の seed 0 と seed 1 の en の組で `vprobe --lang en`（T-RP-37-026 と同じ command）を `<main>` と `<branch>` に打つ | 数と否定の壊した行の止まる数 | `<branch>` が `<main>` 以上 | T-RP-37-026 から 029 が打ち直しの行である。RP-63 は否定の表を変えるので、`break:negation` の `held=` と `ids=` が `<main>` 以上（specified、6 の RP-63 の「数と否定の壊した行の止まる数が今の main 以上」） | 陰性 | - |

陰性の build と落ちる行（表から読んだ予測、NOT VERIFIED）：訳だけの推量も止める今の build は 002 が落ちる。原文の推量を落とした訳を止めない build は 001 が落ちる。8 つを足さない今の build は 004 が落ちる。`ならなかった` を単独で除く build は 007 の (a) が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 001 の (b)(c) と 002 の (c)、006 の en→ja の例は、実装の席が書いた文である。設計の例は `I might go.`、`It will rain.`、`I could go.` までなので、この 4 つの oracle は derived である。
- 002 で出る `translate.hedge.logged` の `lang` の値。`log_pair_hedge` は ja→en 用に `en` を固定で出す形に読め、en→ja で何を出すかは設計に無い。行にしない。
- en→ja の行が `eval` を通るか。6 の RP-63 は「`eval` でも 1 行に 1 件」と書くが、`eval` は `<code>.ja.txt` を原文に読む。en→ja の入口が在るかは確かめていない（NOT VERIFIED）ので、行にしない。
- 004 の「なくては」の形が `ならない`／`いけない` の `ない` を除いた後に残らないこと。除き方は `exclude` の長い順の規則に依り、004 の 8 つが通ることで固定される。

### 4.61 RP-64（欠番）

欠番。D の H66 を見よ（`rust_port.ja.md`。R8u は作らない）。

### 4.62 RP-65（session の言語の文字を持たない 1〜2 字の確定文を出さない）

test：`native/hayamimi/tests/rp65.rs`。受入 test の名前は `rp65_a_one_or_two_letter_final_outside_the_session_script_is_dropped`（001）。`#[ignore]` を持たない。
関数の名前は `rp65_` に各行の「関数の末尾」を続けた物にする。
入力：確定文を出す所に偽の認識の結果（text だけを替える）を渡し、`final` の出力と `asr.final.dropped` を見る。session の言語の文字は、en が Latin、ja が漢字、ひらがな、カタカナ、Latin（6 の RP-65）。`ل` は Arabic の字である。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi --test rp65`。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-65-001 | `--lang en`、text が `ل    `（`ل` と空白 4 つ） | 前後の空白を除くと 1 字で、Latin でない | 捨てる | `final` を出さず、`asr.final.dropped` が 1 件（level info、`reason` が `off_script`）（specified、6 の RP-65 と H67 の観測の行） | 陰性。捨てない今の build で `final` が出て落ちる | `a_one_letter_final_outside_the_session_script_is_dropped` |
| T-RP-65-002 | `--lang en`、text が `لل`（2 字）、`ل`（1 字）、` ل `（空白に挟まれた 1 字） | 1 字と 2 字の境界。空白を除く | 捨てる | 3 つとも `final` を出さず、`asr.final.dropped` が 1 件ずつ（specified、6 の RP-65 の「前後の空白を除いて 1〜2 字」） | 境界、陰性 | `one_and_two_letters_are_dropped_after_trimming` |
| T-RP-65-003 | `--lang en`、text が `للل`（3 字） | 3 字 | 出す | `final` を出し、`asr.final.dropped` が 0 件（specified、6 の RP-65 の「3 字以上の文は今のとおり出す」） | 境界、陽性 | `a_three_letter_final_is_emitted` |
| T-RP-65-004 | `--lang en`、text が `OK`、`I`、`لA`（Latin を含む 2 字） | session の言語の文字を含む 1〜2 字 | 出す | 3 つとも `final` を出し、`asr.final.dropped` が 0 件（specified、6 の RP-65 の「session の言語の文字を含む 1〜2 字の文（en の `OK`）は今のとおり出す」。`I` と `لA` は境界の例） | 陽性。字の数だけで捨てる build で `OK` が消えて落ちる | `a_short_final_with_a_session_letter_is_emitted` |
| T-RP-65-005 | `--lang ja`、text が `はい`、`是`、`ア`、`OK` | ja の文字の 4 種（ひらがな、漢字、カタカナ、Latin） | 出す | 4 つとも `final` を出し、`asr.final.dropped` が 0 件（specified、6 の RP-65 の「ja は漢字、ひらがな、カタカナ、Latin」。`はい` は設計の例） | 陽性。字の数だけで捨てる build で `はい` が消えて落ちる | `a_short_japanese_final_with_a_japanese_letter_is_emitted` |
| T-RP-65-006 | `--lang ja`、text が `ل`、`한` | ja の文字に入らない Arabic と Hangul の 1 字 | 捨てる | 2 つとも `final` を出さず、`asr.final.dropped` が 1 件ずつ（specified、6 の RP-65 の「どの字も session の言語の文字に入らない」） | 陰性 | `a_short_japanese_final_outside_the_japanese_script_is_dropped` |
| T-RP-65-007 | 001 の 1 件の log の形 | log の欄 | 欄の集合 | `level` が info、`reason` が `off_script`。`message` と欄の値に text が入らない（derived、`observability-and-log-monitoring.md` の「入力本文を出力しない」。欄の key は設計が `reason` までを言う） | 境界、陽性 | `the_dropped_event_carries_the_reason_and_not_the_text` |
| T-RP-65-008 | 001 の入力を渡した後、偽の llama-server が受けた要求の数を数える | 捨てた確定文は T に届かない | 要求 0 件 | 要求が 0 件（derived、H67 の観測「T がそれを訳さずに断りの文を返した」の原因が T への依頼である。`final` を出さなければ訳の依頼は出ない） | 境界、陰性 | `a_dropped_final_is_not_sent_to_the_translator` |
| T-RP-65-009 | RP-1 の scorecard の run と golden の 8 本（RP-2）の log で `asr.final.dropped` を数える：`rg -c 'asr.final.dropped' <log>` | 本物の音声の確定文を落とさない | 0 件 | 各 run の log が 0 件（specified、6 の RP-65 の陽性欄）。陽性対照：同じ式で 001 の test の log を数えると 1 以上（式が event を数えている） | 陽性、陰性 | - |

陰性の build と落ちる行（表から読んだ予測、NOT VERIFIED）：字の数だけで捨てる build は 004、005 が落ちる。捨てない今の build は 001、002、006 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 数字と記号だけの 1〜2 字（`42`、`?`、`。`）。「どの字も session の言語の文字に入らない」に当たるなら捨てる。数字を「session の言語の文字」に含むかは設計に無く、期待値を置けない。本物の短い発話（`42`）を落とす穴になり得る（持ち主は設計の席）。
- 空白だけの確定文（除くと 0 字）。「1〜2 字」に 0 字が入るかは設計に無い。
- 009 の run の log の置き場。RP-1 と RP-2 の run が `asr.final.dropped` を残すかは、この席は確かめていない。

### 4.63 E2E-1 と E2E-2（マイクと loopback の実走の記録を判定する）

test：`native/hayamimi-eval/tests/e2e1.rs`（E2E-1）と `native/hayamimi-eval/tests/e2e2.rs`（E2E-2）。受入 test の名前は D の E2E の表（`rust_port.ja.md` の 6 の E2E の小節）どおり、`e2e1_mic_process_ja_to_en_beats_python` と `e2e2_loopback_process_en_to_ja_within_budget` である。ほかの関数の名前は、各行の「関数の末尾」をそのまま使う（`rp` の前置きを持たない。4.1 の 3 つの形のどれでもなく、受入 test の名前の形に揃えた）。
受入 test は `#[ignore]` を持ち、実走の記録（`docs/results/e2e/e2e1/` と `docs/results/e2e/e2e2/`）を読むだけで、何も起こさない。実走の command は D の E2E の表のとおりで、この節は起こさない。受入 test 以外は `#[ignore]` を持たず、`tests/fixtures/` と `docs/results/r8h_verify/` の記録で動く。
入力：`tests/fixtures/e2e_columns.jsonl` は 4 件の標本（`t_end_us` は全部 1000000）。`t_final_us` と `t_tr_us` と `final_latency_ms`（`L_dec`）は 1 件目が 2300000、2500000、1000.0。2 件目が 2300000、2400000、1000.5。3 件目が 2000000、3000000、100.0。4 件目が 1800000、null、100.0。`--min-silence` は 300 ms。有効は `L_asr − L_dec ≥ min_silence`（3.1 の分割式）で、`L_asr = t_final − t_end`、`L = t_tr − t_end`、`L_tr = t_tr − t_final`。
command（受入 test の外）：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test e2e1 --test e2e2`。受入 test は `cargo test --release --test e2e1 -- --ignored`（と `e2e2`）で、記録が在る時だけ打つ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-E2E-1-001 | 実走の記録：Rust の標本 `e2e1.jsonl` と、Python の run の `py<N>.jsonl`（束ねる） | 受入 | 訳の取れた割合 ≥ 20 / 23、`L` の p50 と p95 が Python 以下 | 有効な標本（3.1）のうち訳の取れた割合が 20 / 23 以上で、Rust の `L` の p50 と p95 がともに Python を束ねた値以下。`L`、`L_asr`、`L_tr` の p50 と p95 を出力する（specified、D の E2E-1 の期待値） | 陽性：D の「`<base>` の Python の run から同じ列が出る」は 010 と 013。陰性：訳が 0 件の記録で落ちる（005） | `e2e1_mic_process_ja_to_en_beats_python` |
| T-E2E-1-002 | `e2e_columns.jsonl` を数える | 無効な標本（1 件）と訳の無い標本（1 件）が混ざる | 有効 3、訳あり 2 | 有効 3（2 件目は `1300 − 1000.5 = 299.5 ms` で 300 ms 未満）、訳あり 2（1 件目と 3 件目）。`L` は 1500 と 2000 で n=2、p50 1500、p95 2000。`L_asr` は 800、1000、1300 で n=3、p50 1000、p95 1300。`L_tr` は 200 と 1000 で n=2、p50 200、p95 1000（derived、3.1 の式と nearest-rank の p95 = ceil(0.95 × n) 番目を fixture の数に手で当てた） | 境界（299.5 ms） | `the_columns_count_only_valid_samples` |
| T-E2E-1-003 | 標本が 0 件の run | 空 | 列が無い | 有効 0、訳あり 0、`L` と `L_asr` が無し（specified、3.1 の「標本が無い run は値を持たない」。0 件を速さと読まない） | 境界、陰性 | `a_run_with_no_sample_has_no_column` |
| T-E2E-1-004 | Python は `e2e_columns.jsonl`。Rust は同じ標本の全件に、`t_final_us` の 100000 µs 後の訳を付けた run | Rust が速く、全件に訳が在る | 通る | 判定の失敗が 0 件（derived、有効 3 の全件に訳が在り、`L` は 1400、1100、900 ms で p50 1100 ≤ Python の 1500、p95 1400 ≤ 2000） | 陽性 | `a_run_faster_than_python_and_fully_translated_passes` |
| T-E2E-1-005 | 004 と同じ組で、Rust の訳を `t_final_us` の 1000000 µs 後に付けた run | Rust が遅い | 落ちる | 失敗が 1 件で、p50 は Rust 2000 ms > Python 1500 ms、p95 は Rust 2300 ms > Python 2000 ms の両方を言う（derived、`L` が 2300、2000、1800 ms になる） | 陰性 | `a_run_slower_than_python_fails_on_both_percentiles` |
| T-E2E-1-006 | Python と Rust が同じ `e2e_columns.jsonl` | 百分位が等しい | 等しいは遅くない | 失敗が 1 件で、訳の割合（有効 3 のうち 2）だけを言う。p50 と p95 は失敗に入らない（specified、RP-3 の「p50 と p95 以下」の以下） | 境界 | `equal_percentiles_are_not_slower` |
| T-E2E-1-007 | Rust が `e2e_no_translation.jsonl`（3 件とも訳が無い）。Python は `e2e_columns.jsonl` | 訳が 0 件 | 落ちる | 失敗に `0 of 3 valid samples have a translation` と `rust: no valid sample has a translation` が入る（specified、D の E2E-1 の陰性対照「訳が 0 件で test が落ちる」。llama-server を止めた run の記録の形） | 陰性 | `a_run_with_no_translation_fails` |
| T-E2E-1-008 | Rust の標本が空 | 空の run | 通らない | 失敗に `no valid sample` が入る（specified、空を合格と読まない） | 陰性 | `an_empty_run_is_not_a_pass` |
| T-E2E-1-009 | `docs/results/r8h_verify/` の Rust の `rs1.jsonl` と、Python の `py1.jsonl` と `py2.jsonl`（束ねる） | 実物の記録 | 数が jq と一致 | Rust は有効 40、訳あり 34。Python は有効 80、訳あり 76。出力に `e2e1 rust: 34 of 40 valid samples translated` と `e2e1 python: 76 of 80 valid samples translated` が入る（derived、同じ file を `jq` で数えた値。数え方は test の注釈に在る） | 陽性 | `a_recorded_run_gives_its_columns` |
| T-E2E-1-010 | `r8h_verify` の dir で `rs1` の記録の一覧 | 記録の置き場 | 名前の順 | Rust は `rs1.jsonl`、Python は `py1.jsonl`、`py2.jsonl` の順。`py1.events.jsonl` と `py1.summary.json` は run に数えない（specified、D の E2E-1 の「同じ列を `<base>` の Python の run から出せる」） | 陽性 | `a_run_directory_lists_its_samples_and_the_python_runs_in_name_order` |
| T-E2E-1-011 | 記録の無い run の名前 | 記録が無い | 言う | 失敗が「E2E の席が写す」旨を言う（derived、実走の記録は E2E の席が置く） | 陰性 | `a_run_directory_without_the_samples_says_what_to_copy` |
| T-E2E-1-012 | run の dir の名前 `e2e1` | 置き場の固定 | 末尾が一致 | `docs/results/e2e/e2e1` で終わる（specified、D の E2E の表の記録の置き場） | 境界 | `the_fixed_directory_of_a_run_is_under_docs_results_e2e` |
| T-E2E-1-013 | Python の run が 0 本の記録 | Python が無い | 断る | 失敗が `py<N>.jsonl` の名前を言う（specified、`L` は Python の run と比べて判定する） | 陰性 | `e2e1_without_a_python_run_is_refused` |
| T-E2E-1-014 | 標本の file `/r/obs2.jsonl` | events file の名前 | 隣に在る | `/r/obs2.events.jsonl`（derived、E2E-2 が events を読む規則と同じ） | 境界 | `the_events_file_stands_beside_the_samples_file` |
| T-E2E-1-015 | 有効 80 のうち訳あり 70 の run と 69 の run（Python は別に固定） | 割合の境界 20 / 23 | 70 は通り 69 は落ちる | 70/80 = 0.875 は 20/23 = 0.8696 以上で割合の失敗が 0 件。69/80 = 0.8625 は未満で割合の失敗が 1 件（derived、20/23 の除算。T-RP-43-009 の 70/80 の判定がここに在る）。`L` は Python 以下に揃えて割合だけを動かす | 境界、陽性、陰性 | `the_translated_share_passes_at_70_of_80_and_fails_at_69` |
| T-E2E-2-001 | 実走の記録：Rust の標本 `e2e2.jsonl` と events file `e2e2.events.jsonl` | 受入 | `lang` が en の `final` と ja の訳、割合 ≥ 20 / 23、`L` の p95 ≤ 2000 ms | events に `lang` が `en` の `final` と、`lang` が `ja` の見える訳が在り、訳の取れた割合が 20 / 23 以上で、`L` の p95 が 2000 ms 以下。p50 は出力するが判定しない（specified、D の E2E-2 の期待値） | 陽性：`/ingest` の run（008）。陰性：訳が 0 件の記録で落ちる（009） | `e2e2_loopback_process_en_to_ja_within_budget` |
| T-E2E-2-002 | 予算 `E2E2_P95_BUDGET_MS` と、`L` の p95 が 1999、2000、2001 ms の列 | 予算の境界 | 2000 は通り 2001 は落ちる | 予算は 2000。1999 と 2000 は失敗なし。2001 は `L p95 2001 ms > 2000 ms` を言う（specified、RP-3 の en→ja の「p95 が 2000 ms 以下」） | 境界、陰性 | `the_budget_is_2000_ms_and_the_boundary_passes` |
| T-E2E-2-003 | `L` が無い列（標本が無い） | `L` 無し | 通らない | 予算の失敗が在る（specified、空を予算の内と読まない） | 陰性 | `a_run_with_no_l_is_over_no_budget_and_still_fails` |
| T-E2E-2-004 | 9 行の events：en の `final` 2 件、ja の `final` 1 件、ja の見える訳 2 件（`status` が `ok`）、ほかに `status` が無い訳、en の訳、`held` の訳、空の訳、`of` が `partial` の訳 | 数える対象の仕分け | en の `final` 2、ja の見える訳 2 | en の `final` が 2、ja の見える訳が 2。ja の `final`、en の訳、`status` が `ok` でない訳、空の訳、`partial` の訳は数えない（specified、D の E2E-2 の「`lang` が `en` の `final` と `lang` が `ja` の訳」と、字幕に見える訳だけを数える H の規則） | 境界、陰性 | `the_events_are_counted_by_language_and_by_what_the_subtitle_shows` |
| T-E2E-2-005 | events の行が `not json`、`{"t_us":1}`、空の文字列 | 不正な行 | 断る | 前の 2 つは失敗、空は (0, 0)（derived） | 陰性、境界 | `an_events_line_that_is_not_an_event_is_refused` |
| T-E2E-2-006 | 全件に訳が在り速い run と、en の `final` と ja の訳が 1 件ずつの events | 通る | 通る | 判定の失敗が 0 件（derived） | 陽性 | `a_fast_translated_run_with_both_languages_passes` |
| T-E2E-2-007 | `L` が 2300、2000、1800 ms の run | p95 だけが予算を超える | 1 件だけ落ちる | p95 は最大の 2300 ms。失敗が 1 件で `L p95 2300 ms > 2000 ms`（derived、nearest-rank の p95 = ceil(0.95 × 3) = 3 番目） | 陰性 | `a_run_whose_p95_is_over_2000_ms_fails_on_that_alone` |
| T-E2E-2-008 | `r8h_verify` の `/ingest` の run `e2e2c.jsonl` と `e2e2c.events.jsonl` | 実物の記録 | 数が jq と一致 | 有効 30、訳あり 25、en の `final` 38、ja の見える訳 54。出力に `e2e2 rust: 25 of 30 valid samples translated` と `38 final in en, 54 shown translation in ja` が入る（derived、同じ file を `jq` で数えた値。D の E2E-2 の陽性対照「`/ingest` の run でも同じ列が出る」） | 陽性 | `a_recorded_run_gives_its_columns_and_event_counts` |
| T-E2E-2-009 | 訳が 0 件の記録（`e2e_no_translation.jsonl`） | 訳が 0 件 | 落ちる | 失敗に `0 of 3 valid samples have a translation`、`rust: no valid sample has a translation`、`no shown translation in ja` が入り、`no final in en` は入らない（specified、D の E2E-2 の期待値。en の `final` は在り ja の訳だけが無い） | 陰性 | `a_run_with_no_translation_fails` |
| T-E2E-2-010 | 標本も events も空の run | 空 | 全部の数で落ちる | 失敗に `no valid sample`、`hold no final in en`、`hold no shown translation in ja` が入る（specified、空を合格と読まない） | 陰性、境界 | `an_empty_run_with_no_events_fails_on_every_count` |
| T-E2E-2-011 | 割合の境界：有効 23 のうち訳あり 20 の run と 19 の run | 20 / 23 の境界 | 20 は通り 19 は落ちる | 20/23 は割合の失敗が 0 件、19/23 は 1 件。events と `L` は通る値に揃える（derived、20/23 と同じ除算） | 境界、陰性 | `the_translated_share_boundary_is_20_of_23` |

実装済みの行は、E2E-1 の 001 から 015 と E2E-2 の 001 から 011 である。015 は `native/hayamimi-eval/tests/e2e1.rs`、E2E-2-011 は `native/hayamimi-eval/tests/e2e2.rs` に在る。
陰性の build と落ちる行（表から読んだ予測、NOT VERIFIED）：割合を数えない build は 007、009、015、E2E-2-011 が落ちる。`L` の p50 だけを判定する build は 005 が落ちる。0 件を合格と読む build は 003、008、E2E-2-003、010 が落ちる。ja の `final` を数える build は E2E-2-004 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- D の E2E-1 の陰性対照のうち「`hayamimi` と中継は止まらず `translate.fallback.held` を出す」と、E2E-2 の「出力の装置を止めた run では止まらず `capture.fallback.none` が 1 件」は、記録を読む test の外の確かめで、この節の行にしていない。機械で確かめる在り処は設計に無い（持ち主は E2E の席）。
- 受入 test は記録を読み、何も起こさない。D の E2E の表の command（llama-server、中継、process A と B を起こす）を、誰がどの手番で打って記録を置くか（`docs/results/e2e/` への写し）は、設計の 8 の R8 の行に在るが、この節は触れない。
- 009 と E2E-2-008 の数は `jq` で数えた値と注釈に在るだけで、式が test に無い。記録を入れ替えると値が動く。式を test の定数と一緒に置くかは実装の席が決める。
- 015 と E2E-2-011 は、`judge_e2e1` と `judge_e2e2` の割合の境界の行である。今の test は 3 件の fixture の割合（2 of 3）でしか割合を打っておらず、20 / 23 の境界は実走の記録でしか確かめていない。

### 4.64 RP-66（発話の終わりの待ち `d` と区切りの品質）

**未実装。** この節の行はどれも、test の file も関数も repo に無い。実装の席が R8v の手番で足す。
test：`native/hayamimi-eval/tests/rp66.rs`。受入 test の名前は `rp66_early_end_shortens_the_wait_without_splitting_utterances`。実走の行（T-RP-66-038）だけが `#[ignore]` を持ち、`docs/results/r8v/` の記録を読む。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp66`。実走の行はこれに `-- --ignored` を足す。
関数の名前と引数の形は実装の席が決める。行は「言語ごとに、main の run と R8v の run の対を渡す」形で書く。

**数え方**（3.1 と 3.17 のとおり。この席は足さない）：

- 標本の枠は ja が 23 × 5 = 115、en が 15 × 5 = 75。枠は 有効 + final なし + 終端不正 に分かれる。RP-66 は終端不正の線を 0.12 秒にして数える（3.1 の例外）。
- `d = L_asr − L_dec`（ms）。有効は `final` が在り `d ≥ 120`。0.30 秒で数える有効は `d ≥ 300`。
- p50、p95 は nearest-rank（順位は (p × n + 99) / 100 の整数除算）。`d` の n は、訳の有無を問わない有効な標本の数である（D 3.17 の「RP-66 と RP-67 の数え方」の 1）。「final なし」は `t_final_us` が null の標本（同 2）。`d` の合否と品質の合否は、言語ごとに R8v の 2 本を合わせた集まりと、main の 2 本を合わせた集まりで取る（同 7）。run ごとの値は記録だけにする。
- 合否は 2 つ（6 の RP-66）。(1) `d` の 4 つの限度。(2) 品質：文の途中の切断の率（上限以下）と、「final なし」の数と訳の取れない有効な標本の数と ja の CER（対の main より悪くならない）。`finals` が 2 以上の標本の数、仕分けの 3 つの数、割れた標本の訳の chrF、`L`、復号、訳は記録だけ。文の途中の切断の率の上限の値は、評議が手番 1b の後、手番 1c の前に固定する。この節は数を置かない。
- B だけが作った割れは、3 つに仕分ける：本物の文の切れ目、尾の雑音、文の途中（D 3.17 の数え方の 8）。en の参照の文の終わりの印は、A の final の句読点から写す（同 9）。発話の終わりの遅れは最後の final で取る（同 10）。

**fixture**（式で作る。実測のふりをしない）：

- **F66J**（ja の R8v の run）：枠 115 = 有効 100 + final なし 15。有効の k 番目（k = 1〜100、`d` の小さい順）の `d` は、k ≤ 50 で 450 + k、51 ≤ k ≤ 95 で 1050 − (95 − k)、96 ≤ k で 1100 + 10 × (k − 96)。したがって 50 番目が 500 ms、95 番目が 1050 ms で、nearest-rank の p50 は 50 番目 ((50 × 100 + 99) / 100)、p95 は 95 番目 ((95 × 100 + 99) / 100) である。有効の標本は全部 `L_dec` 100 ms、`L_tr` 300 ms、`finals` 1、`cer_dist` 1、`cer_denom` 50。final なしの 15 は `t_final_us` が null、`finals` 0、`cer_dist` 1、`cer_denom` 50。
- **F66E**（en の R8v の run）：枠 75 = 有効 75。k 番目の `d` は、k ≤ 38 で 262 + k、39 ≤ k ≤ 72 で 520 − (72 − k)、73 ≤ k で 600 + 10 × (k − 73)。38 番目が 300 ms、72 番目が 520 ms で、p50 は 38 番目 ((50 × 75 + 99) / 100)、p95 は 72 番目 ((95 × 75 + 99) / 100) である。`L_dec` 200 ms、`L_tr` 200 ms、`finals` 1、`cer_dist` 1、`cer_denom` 50。
- **M66J、M66E**（main の run）：F66J の全標本の `d` に 400 ms を足した物（M66J。50 番目が 900 ms）、F66E に 150 ms を足した物（M66E。38 番目が 450 ms）。ほかの欄は F66 と同じ。900 ms と 450 ms は、記録の main の `d` の p50（ja 865 ms、en 457 ms。D 3.17 の数え方の 1）の ±60 ms の帯（ja 805〜925、en 397〜517）に入る。
- **対**：`[M66, F66, M66, F66]`（main、R8v、main、R8v の順。2 つの R8v の run は同じ標本、2 つの main の run も同じ標本）。load の記録は全 run の前で `0.50`。順と load は RP-32 と同じ規則で、行 028〜031 が持つ。

合わせた集まりは、ja が n 200（p50 は 100 番目、p95 は 190 番目）、en が n 150（p50 は 75 番目、p95 は 143 番目）である。2 本が同じ標本なので、k 番目の値は合わせた順位の 2k − 1 番目と 2k 番目に並ぶ（ja の p50 は k = 50、p95 は k = 95。en の p50 は k = 38、p95 は k = 72）。
行 001〜009 の「R8v の k 番目を変える」は、2 本の R8v の run の両方に当てる。品質の行（016〜025）の「R8v の標本を 1 つ変える」は、2 本のうち 1 本にだけ当てる（合わせた数で 1 件の差になる）。

**仕分けの fixture**（D 3.17 の数え方の 8 と 9。字や語で作る。実測のふりをしない）：

- **参照 J5**（ja）：`今日は天気がいいですね。明日は雨です。`。NFKC の後に空白と句読点を除くと `今日は天気がいいですね明日は雨です`（17 字）で、`。` は 11 字目の後と 17 字目の後に在る。**参照 J4**：`今日は天気がいいですね、明日は雨です。`（`、` は終わりの印でない。`。` は 17 字目の後だけ）。
- 割れの「前の文」は参照の先頭 e 字、「後ろの文」は残り。e = 11 は参照の文の切れ目にちょうど当たる。
- **参照 E**（en）：`the cat sat on the mat and then it left`（10 語）。**A の final の文 E1**：`The cat sat on the mat. And then it left.`（`.` は 6 語目と 10 語目の後）。**E0**：`The cat sat on the mat and then it left.`（`.` は 10 語目の後だけ）。割れの前の文は先頭 e 語。
- 仕分けの優先は、設計が書かない（本物の文の切れ目と尾の雑音の両方に当たる形、たとえば J5 の末尾の割れ）。この fixture は両方に当たる形を使わない（この節の「設計にない物」）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-66-001 | ja は `[M66J, F66J, M66J, F66J]`、en は `[M66E, F66E, M66E, F66E]`。そのまま | 4 つの限度がどれも等号 | `Ok` | ja も en も `Ok`。ja の `d` は p50 500 ms、p95 1050 ms、en は p50 300 ms、p95 520 ms（specified、6 の RP-66 の「以下」。F66 の式） | 境界、陽性 | `d_at_the_four_limits_passes` |
| T-RP-66-002 | 001 の ja で、R8v の 50 番目を 501 ms にする | ja の p50 の順位に当たる標本 | `Err` | `Err` で ja の p50 だけを名指し。p95 の 95 番目は 1050 ms のまま（specified、6 の RP-66。ja p50 ≤ 500）。1050 ms の置き直しで書き直す（D 3.17 の数え方の 5） | 境界、陰性 | `ja_p50_of_d_one_millisecond_over_is_rejected` |
| T-RP-66-003 | 001 の ja で、R8v の 51 番目だけを +1 ms にする | p50 の順位の次 | `Ok` | `Ok`。50 番目は 500 ms のまま（derived、nearest-rank。順位が p50 を決める）。1050 ms の置き直しで書き直す | 境界 | `ja_p50_is_decided_by_the_50th_sample_only` |
| T-RP-66-004 | 001 の ja で、R8v の 95 番目を 1051 ms にする | ja の p95 の順位に当たる標本 | `Err` | `Err` で ja の p95 だけを名指し。p50 は通る（specified、6 の RP-66。ja p95 ≤ 1050）。1050 ms の置き直しで書き直す（D 3.17 の数え方の 5） | 境界、陰性 | `ja_p95_of_d_one_millisecond_over_is_rejected` |
| T-RP-66-005 | 001 の ja で、R8v の 96 番目だけを +1 ms にする | p95 の順位の次 | `Ok` | `Ok`。95 番目は 1050 ms のまま（derived、nearest-rank）。1050 ms の置き直しで書き直す | 境界 | `ja_p95_is_decided_by_the_95th_sample_only` |
| T-RP-66-006 | 001 の en で、R8v の 38 番目を 301 ms にする | en の p50 の順位に当たる標本 | `Err` | `Err` で en の p50 だけを名指し（specified、6 の RP-66。en p50 ≤ 300） | 境界、陰性 | `en_p50_of_d_one_millisecond_over_is_rejected` |
| T-RP-66-007 | 001 の en で、R8v の 39 番目だけを +1 ms にする | p50 の順位の次 | `Ok` | `Ok`。38 番目は 300 ms のまま（derived、nearest-rank） | 境界 | `en_p50_is_decided_by_the_38th_sample_only` |
| T-RP-66-008 | 001 の en で、R8v の 72 番目を 521 ms にする | en の p95 の順位に当たる標本 | `Err` | `Err` で en の p95 だけを名指し（specified、6 の RP-66。en p95 ≤ 520） | 境界、陰性 | `en_p95_of_d_one_millisecond_over_is_rejected` |
| T-RP-66-009 | 001 の en で、R8v の 73 番目だけを +1 ms にする | p95 の順位の次 | `Ok` | `Ok`。72 番目は 520 ms のまま（derived、nearest-rank） | 境界 | `en_p95_is_decided_by_the_72nd_sample_only` |
| T-RP-66-010 | **S2 の陽性対照**：main の run 2 本（M66J と M66E）。実物の記録として `docs/results/e2e/e2e1/e2e1.jsonl`（枠 115）と `docs/results/e2e/e2e2/e2e2.jsonl`（枠 75）も渡す（git の中に在る。写しを作らない。D 3.17 の数え方の 6） | main の run は早い規則を持たない | 0.12 秒と 0.30 秒の有効の数が一致 | M66J は 100 と 100、M66E は 75 と 75。実物は ja が 100 と 100、en が 75 と 75（pseudo、jq。`select(.t_final_us != null and ((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) >= 120)` と `>= 300` の数。2026-10-15 に打った）。一致していれば `Ok`。出力に 2 つの数を並べる | 陽性、境界 | `main_runs_count_the_same_valid_samples_at_120_and_300_ms` |
| T-RP-66-011 | **S2 の陰性対照**：M66J の 1 標本の `d` を 250 ms にした main の run | main の run で 2 つの数が違う | `Err` | `Err` で main の run の有効の数が 0.12 秒と 0.30 秒で合わないことを名指し（0.12 秒で 100、0.30 秒で 99）（derived、main は早い規則を持たず、`d` の最小は ja 428 ms、en 361 ms なので、2 つの数は等しいはずである。3.17 の記録） | 陰性 | `a_main_run_whose_valid_count_differs_between_120_and_300_ms_is_rejected` |
| T-RP-66-012 | **S3**：F66J の標本の `d` を 119、120、121、299、300、301 ms にした 6 標本の run（`final` は在る） | 0.12 秒でだけ有効になる標本。両側の境界 | 数と分布を別に記録 | 0.30 秒の有効は 2（300、301）、0.12 秒の有効は 5。0.12 秒でだけ有効なのは 3（120、121、299）。その `d` の最小 120、p50 121 (((50 × 3) + 99) / 100 = 2 番目)、p95 299 ((95 × 3 + 99) / 100 = 3 番目)、最大 299 を出力に別の欄で書く（derived、手で数えた。119 は両方で終端不正、300 は両方で有効） | 境界、陽性 | `samples_valid_only_at_120_ms_are_counted_with_their_d` |
| T-RP-66-013 | S3：M66J と M66E、および 010 の実物の 2 本 | 0.12 秒でだけ有効な標本が無い | 数 0、分布は欄を持たない | 数は 0。分布の欄は値を持たない（0 と書かない）。`Ok`（derived、010 の数の差 0。0 件を「待ちが 0 ms」と読ませない） | 陰性 | `no_sample_valid_only_at_120_ms_gives_zero_and_no_distribution` |
| T-RP-66-014 | F66J の 1 番目の標本の `d` を 250 ms にした R8v の run（対は 001 と同じ） | H70。0.12 秒で有効、0.30 秒で終端不正 | `Ok` | `Ok`。250 ms の標本は有効に入り、有効は 100、p50 は 50 番目の 500 ms のまま。0.30 秒で数えた場合は有効が 99 になり、50 番目が 1006 ms になって ja の p50 の限度を超える（derived、F66J の 51 番目が 1006 ms。したがって 0.30 秒で数える build はこの行で落ちる） | 陽性、陰性 | `a_sample_with_a_wait_under_300_ms_counts_as_valid_at_120_ms` |
| T-RP-66-015 | F66J の 1 番目の `d` を 100 ms にした run、250 ms にした run、そのままの run | 枠が三つに分かれる。0.12 秒の線の両側 | 合計が枠 | 100 ms の run は 0.12 秒で 有効 99 + final なし 15 + 終端不正 1 = 115、0.30 秒で 99 + 15 + 1 = 115。250 ms の run は 0.12 秒で 100 + 15 + 0、0.30 秒で 99 + 15 + 1。どの組も 115 になる。そのままの run は 100 + 15 + 0 の 2 通りとも（derived、3.1 の「枠 = 有効 + final なし + 終端不正」。標本を落として合計が枠を割ると `Err`） | 境界、陽性 | `the_frame_splits_into_valid_no_final_and_bad_end_without_loss` |
| T-RP-66-016 | **S1**：001 の ja で、R8v の final なしの 1 標本を `finals` 2 にする（`t_final_us` は null のまま） | 割れた標本が有効でない（枠の中）。記録の項目 | `Ok` と数 | `Ok`。記録の `finals` が 2 以上の標本の数は main 0、R8v 1。有効な標本だけで数えると 0 になり、記録が欠ける（derived、D 3.17 の数え方の 3。枠の全標本で数える。6 の RP-66 は `finals` が 2 以上の標本を合否に使わない） | 陽性、陰性 | `a_split_sample_without_a_final_is_recorded_over_the_whole_frame` |
| T-RP-66-017 | 001 の ja で、R8v の有効な標本の 1 つを `finals` 2 にする | 割れた標本が 1 件増える | `Ok` と数 | `Ok`。記録の数は main 0、R8v 1 で、増えても落ちない（specified、6 の RP-66 の「記録するだけで合否に入れない」。旧い判定は増えると `Err` だった。常に完結と判定する build は、行 056 から 058 の文の途中の切断の率で落ちる） | 陰性 | `one_more_split_sample_is_recorded_and_does_not_fail` |
| T-RP-66-018 | main は 2 本それぞれ `finals` が 2 の標本を 1 つ（合わせて 2）。R8v は合わせて 2 と 1 と 5 | 割れた標本の数が等しい、減る、増える | すべて `Ok` | 3 つとも `Ok`。記録の数は R8v が 2、1、5（specified、同上。実物の記録は ja が 10、en が 15 の標本を持つ。jq の `select(.finals >= 2)` を枠の全標本に当てた数） | 境界、陽性 | `split_samples_are_recorded_whichever_way_the_count_moves` |
| T-RP-66-019 | 001 の ja で、R8v の有効な標本の 1 つを final なしにする（`t_final_us` が null、`finals` 0） | final なしが 1 件増える | `Err` | `Err` で final なしの標本の数を名指し（2 本合わせて main 30、R8v 31）。有効は 99 になる（specified、6 の RP-66。増えない。枠 115 の全標本で数える） | 陰性 | `one_more_sample_without_a_final_is_rejected` |
| T-RP-66-020 | main の final なしが 2 本合わせて 30、R8v が 30 と 29 | final なしが等しい、減る | `Ok` | どちらも `Ok`（specified、6 の RP-66 の「増えず」。等号側） | 境界 | `equal_or_fewer_samples_without_a_final_pass` |
| T-RP-66-021 | 001 の ja で、R8v の有効な標本の 1 つの `t_tr_us` を null にする | 訳の取れない有効な標本が 1 件増える | `Err` | `Err` で訳の取れない有効な標本の数を名指し（main 0、R8v 1）（specified、6 の RP-66。増えない） | 陰性 | `one_more_valid_sample_without_a_translation_is_rejected` |
| T-RP-66-022 | main の訳の取れない有効な標本が 2 本合わせて 2、R8v が 2 と 1 | 等しい、減る | `Ok` | どちらも `Ok`（specified、6 の RP-66 の「増えず」。等号側。実物の記録は ja が 7、en が 5） | 境界 | `equal_or_fewer_valid_samples_without_a_translation_pass` |
| T-RP-66-023 | ja。main の 2 本を合わせた `cer_dist` の和 230、`cer_denom` の和 11500（F66J の全 115 標本が 2 本）。R8v は合わせた和 230 と 229 | CER が等しい、小さい | `Ok` | どちらも `Ok`（specified、6 の RP-66 の「対の main 以下」。等号側。CER は和 ÷ 和。等号の 230 / 11500 を割り算の丸めで外さない） | 境界、陽性 | `ja_cer_equal_to_or_below_main_passes` |
| T-RP-66-024 | ja。R8v の合わせた `cer_dist` の和 231 の 2 入力。(1) 有効な標本の 1 つの `cer_dist` を 2 にする。(2) final なしの標本の 1 つの `cer_dist` を 2 にする | CER が 1 つ増える。final なしの標本も数える | どちらも `Err` | どちらも `Err` で ja の CER を名指し（specified、6 の RP-66。「run の全標本」の和）。(2) は有効な標本だけで和を取ると通ってしまう。落ちた入力は assert の message に出す | 境界、陰性 | `ja_cer_above_main_is_rejected_whichever_sample_carries_it` |
| T-RP-66-025 | en。R8v の `cer_dist` の和が main より大きい対 | en の CER | `Ok` | `Ok`。en の CER は判定しない（specified、6 の RP-66 は「ja の CER」とだけ書く） | 陰性 | `english_cer_is_not_judged` |
| T-RP-66-026 | F66J の run | 記録するだけの列 | 6 つの値 | 出力に `L`、復号、訳の p50 と p95 が在る。`L` は p50 900 ms、p95 1450 ms（有効で訳の取れた 100 標本の 50、95 番目。`L = d + L_dec + L_tr` で `d` が 500、1050 のとき 500 + 100 + 300、1050 + 100 + 300）、復号は 100 ms と 100 ms、訳は 300 ms と 300 ms（derived、F66 の式。D の 3.17 の jq と同じ分け方）。F66E は `L` が 700 ms と 920 ms（300 + 200 + 200、520 + 200 + 200 の 38、72 番目）、復号は 200 ms と 200 ms、訳は 200 ms と 200 ms | 陽性 | `l_decode_and_translation_are_recorded` |
| T-RP-66-027 | 001 の ja で、R8v の全標本の `L_tr` を 5300 ms にする | 訳が遅い | `Ok` | `Ok`。`d` は動かず、`L` の p95 と訳の p95 が大きくなっても落ちない。出力の `L` の p95 は 6450 ms、訳の p95 は 5300 ms（specified、6 の RP-66 の「`L`、復号、訳は p50 と p95 を記録するだけで合否に入れない」） | 陰性 | `a_slow_translation_does_not_fail_the_wait_verdict` |
| T-RP-66-028 | 001 の ja で、順を main、main、R8v、R8v、にする | 順が交互でない | `Err` | `Err` で 2 番目の run を名指し（specified、6 の RP-66 の「main、R8v、main、R8v の順」。RP-32 の T-RP-32-002 と同じ形） | 陰性 | `a_run_order_that_is_not_alternated_is_rejected` |
| T-RP-66-039 | ja。R8v の 1 本目は全標本の `d` を 460 ms にした run（有効 100）、2 本目は F66J の 50 番目だけ 501 ms にした run。main は M66J の 2 本 | 2 本を合わせると通り、2 本目だけだと落ちる | `Ok` | `Ok`。合わせた n 200 の p50 は 100 番目で 460 ms（1 本目の 100 標本が 500 ms 以下）、p95 は 190 番目で 2 本目の 90 番目の 1045 ms（derived、D 3.17 の数え方の 7）。2 本目だけで取ると p50 が 501 ms で `Err` になる。run ごとに判定する build はここで落ちる。run ごとの p50 と p95（460 ms と 460 ms、501 ms と 1050 ms）は記録だけに出す | 陽性、陰性 | `two_runs_are_pooled_per_language_for_d` |
| T-RP-66-040 | **仕分け（ja、本物の文の切れ目）**：B だけの割れ 1 件。参照 J5、前の文は先頭 11 字（e = 11）、後ろの文は残りの 6 字 | 合わせた終わりの直後に `。` | 本物の文の切れ目 | 本物 1、文の途中 0、尾の雑音 0（specified、D 3.17 の数え方の 8。`。` が 11 字目の直後に在る） | 陽性 | `a_split_at_a_reference_sentence_end_is_a_real_boundary` |
| T-RP-66-041 | 040 の e を 10 にする（前の文は `今日は天気がいいです`） | 終わりの 1 字手前（+1 側の境界） | 本物の文の切れ目 | 本物 1（specified、同上の「直後（±1 字）」。`。` は合わせた終わりの 1 字後） | 境界 | `a_split_one_character_before_the_mark_is_still_a_real_boundary` |
| T-RP-66-042 | 040 の e を 12 にする（前の文は `今日は天気がいいですね明`） | 終わりの 1 字後（−1 側の境界） | 本物の文の切れ目 | 本物 1（specified、同上。`。` は合わせた終わりの 1 字前） | 境界 | `a_split_one_character_after_the_mark_is_still_a_real_boundary` |
| T-RP-66-043 | 040 の e を 9 にする（前の文は `今日は天気がいいで`） | 印から 2 字手前（外側の境界） | 文の途中 | 文の途中 1（specified、同上。`。` が合わせた終わりの 2 字後で ±1 字の外。後ろの文は参照の残りに合うので尾の雑音でない） | 境界、陰性 | `a_split_two_characters_before_the_mark_is_mid_sentence` |
| T-RP-66-044 | 040 の e を 13 にする（前の文は `今日は天気がいいですね明日`） | 印から 2 字後（外側の境界） | 文の途中 | 文の途中 1（specified、同上。`。` が合わせた終わりの 2 字前。後ろの文 `は雨です` は参照の残りに合う） | 境界、陰性 | `a_split_two_characters_after_the_mark_is_mid_sentence` |
| T-RP-66-045 | **尾の雑音**：参照 J4、前の文は先頭 11 字、後ろの文は `ん` | 後ろの文の字が参照の残り（`明日は雨です`）に合わない。直後は `、` で終わりの印でない | 尾の雑音 | 尾の雑音 1、本物 0、文の途中 0（specified、D 3.17 の数え方の 8。`ん` は残りの字のどれにも合わず、`、` は印でない） | 陽性 | `a_trailing_character_that_matches_nothing_is_tail_noise` |
| T-RP-66-046 | 045 の後ろの文を `。`（句読点だけ）にする | 除くと空 | 尾の雑音 | 尾の雑音 1（specified、同上の「同じ形で除いて空」） | 境界 | `a_trailing_sentence_that_is_empty_after_stripping_is_tail_noise` |
| T-RP-66-047 | 040、043、045 の 3 件を 1 つの run に入れる。040 をもう 1 件足す（本物が 2 件） | 3 つの数の和 | 4 件 | 本物 2、文の途中 1、尾の雑音 1。和は割れの数 4 に等しい。どの割れも 3 つのどれか 1 つだけに入る（derived、D の数え方の 8 の「3 つのどれか 1 つに入れる」）。数は 3 つの別の欄で出力する | 陽性、境界 | `every_split_is_in_exactly_one_of_the_three_kinds` |
| T-RP-66-048 | 割れが 0 件の run（有効 100）と、有効が 0 件の run | 割れ無し、母数無し | 前者は 3 つが 0、後者は `Err` | 前者は 3 つの数が 0 で率が 0（specified）。後者は `Err` で有効が 0 件であることを名指し。0 で割った値を率として出さない（derived、0 件を成功と読まない） | 陰性、境界 | `no_split_gives_zero_and_no_valid_sample_is_rejected` |
| T-RP-66-049 | **仕分け（en）**：割れ 1 件。参照 E、A の final E1、前の文は先頭 6 語、後ろの文は残り | 合わせた終わりの直後に A の final の `.` | 本物の文の切れ目 | 本物 1（specified、D 3.17 の数え方の 9。`.` を参照の 6 語目の後へ写す） | 陽性 | `an_en_split_at_the_mark_of_the_final_sentence_is_a_real_boundary` |
| T-RP-66-050 | 049 の前の文を先頭 5 語、7 語にした 2 件 | ±1 語の内側 | 本物の文の切れ目 | 2 件とも本物（specified、同上の「±1 語」） | 境界 | `an_en_split_one_word_either_side_of_the_mark_is_a_real_boundary` |
| T-RP-66-051 | 049 の前の文を先頭 4 語、8 語にした 2 件 | ±1 語の外側 | 文の途中 | 2 件とも文の途中（specified、同上。後ろの文は参照の残りに合う） | 境界、陰性 | `an_en_split_two_words_from_the_mark_is_mid_sentence` |
| T-RP-66-052 | A の final を E0 にし、前の文を先頭 6 語、後ろの文を `uh` にする | A の final の `.` が末尾にしか無い | 尾の雑音 | 尾の雑音 1、本物 0（specified、同上。6 語目の後に印が無く、`uh` は残りに合わない） | 陽性、陰性 | `an_en_tail_word_with_no_mark_nearby_is_tail_noise` |
| T-RP-66-053 | A の final を `The cat sat on the map. And then it left.`（`mat` を `map` と誤認）にし、参照 E に写す | 認識の誤りを含む合わせ | 印の位置 | `.` が参照の 6 語目の後と 10 語目の後に写る。編集距離の最小の合わせで `map` と `mat` が対になる（derived、D の数え方の 9 の「数え方の 8 の合わせ」） | 陽性 | `marks_of_the_final_sentence_are_carried_over_through_the_edit_alignment` |
| T-RP-66-054 | **en の手の確かめ（ja）**：割れ 5 件。(1) J5、e = 11、A の final は J5 と同じ。(2) J5、e = 11、A の final は `今日は天気がいいですね明日は雨です。`。(3) J5、e = 13、A の final は `今日は天気がいいですね明日。は雨です。`。(4) J5、e = 9、A の final は J5 と同じ。(5) J4、e = 11、後ろの文 `ん`、A の final は J4 と同じ | 参照の `。` による仕分けと A の final の `。` による仕分けの一致 | 一致 3、不一致 2 | 一致 3 件（(1)(4)(5)）、不一致 2 件（(2)(3)）を別の欄に出す。(2) は参照で本物、A で文の途中。(3) は参照で文の途中、A で本物。不一致が多い時の上限は評議が手番 1b の後に固定する。この行は上限と比べず、2 つの数だけを確かめる（specified、D の数え方の 9。一致の件数と一致しなかった件数を並べて記録する） | 陽性、陰性 | `reference_and_final_sentence_sortings_are_compared_and_both_counts_are_written` |
| T-RP-66-055 | 054 の 5 件を、A の final を全部参照と同じにして仕分ける | 全て一致 | 一致 5、不一致 0 | 一致 5、不一致 0（derived。054 で不一致 2 を数えた同じ道具が、一致する入力で 0 を返す対照） | 陰性、境界 | `identical_sortings_give_no_disagreement` |
| T-RP-66-056 | **文の途中の切断の率**：ja の R8v を合わせた有効 200 標本（F66J の 2 本）に、文の途中の割れ 3 件と本物 2 件と尾の雑音 1 件。もう 1 つの run は文の途中 0 件 | 率の式 | 0.015 と 0 | 率 = 3 ÷ 200 = 0.015。0 件の run は 0（derived、D 6 の RP-66 の「文の途中と仕分けた割れの数 ÷ 有効な標本の数」。本物と尾の雑音は分子に入らない）。この行は率の値だけを確かめる。判定は 057 | 陽性、境界 | `the_mid_sentence_cut_rate_counts_only_mid_sentence_splits_over_valid_samples` |
| T-RP-66-057 | 上限 U を置いた後の 056 の形。文の途中の割れが ⌊U × 200⌋ 件の run と、1 件多い run | 上限の両側 | 前者は `Ok`、後者は `Err` | 前者は `Ok`（率 ≤ U の等号側）、後者は `Err` で文の途中の切断の率を名指し。U の値は評議が手番 1b の後に固定する。値が固定されるまで、この行の数は書かない（specified、D 6 の RP-66 の「上限以下」） | 境界、陰性 | `the_mid_sentence_cut_rate_limit_is_inclusive` |
| T-RP-66-058 | 完結の判定を常に真にした build の run を模した入力。文の途中の割れが ⌊U × 200⌋ + 1 件以上 | 毎回の候補で確定する | `Err` | `Err` で文の途中の切断の率を名指し。割れの数（`finals` が 2 以上の標本の数）は記録だけで、これでは落ちない（specified、D 6 の RP-66 の陰性欄）。U は評議が手番 1b の後に固定する | 陰性 | `an_always_complete_build_fails_on_the_mid_sentence_cut_rate` |
| T-RP-66-059 | 割れた標本の訳の chrF：(1) 前の文の訳と後ろの文の訳をつないだ物が、A の 1 つの final の訳と同じ文字列。(2) 共通する字が 1 つも無い文字列 | 記録だけの欄 | 100 と 0。判定に入らない | (1) の chrF は 100、(2) は 0 を出力に書く。どちらの値でも他の判定は変わらず `Ok` のまま（derived、chrF の定義。D の数え方の 8 は「合否に入れない」） | 陽性、境界 | `the_chrf_of_split_translations_is_recorded_and_not_judged` |
| T-RP-66-060 | **最後の final の遅れ**：10 発話。発話 k の `end_sample` は 10 秒、k = 1〜10 の最後の final の検出が 100k ms 後。ただし発話 3 は final が 2 つで、最初が 300 ms 後、最後（B だけの割れの後ろの文）が 800 ms 後 | 割れた発話の遅れ | p50 600、p95 1000 | 遅れの列は 100、200、800、400、500、600、700、800、900、1000。小さい順に 100、200、400、500、600、700、800、800、900、1000。p50 は (50 × 10 + 99) / 100 = 5 番目で 600 ms、p95 は (95 × 10 + 99) / 100 = 10 番目で 1000 ms（derived、D の数え方の 10。最初の final の 300 ms で取ると p50 は 500 ms になる） | 陽性、陰性 | `delay_is_taken_at_the_last_final_of_the_utterance` |
| T-RP-66-061 | 060 の発話 3 を final 1 つ（300 ms 後）にした run | 割れの無い発話 | p50 500、p95 1000 | 遅れの列は 100〜1000 の 10 個。p50 は 5 番目の 500 ms、p95 は 10 番目の 1000 ms（derived）。060 との差が最後の final の効き目 | 境界 | `an_unsplit_utterance_is_delayed_at_its_only_final` |
| T-RP-66-062 | 060 の発話 10 を、A の segment が 1 つも触れない発話にする | 母数から外す発話 | n 9、除外 1 | n は 9。遅れの列は 100、200、800、400、500、600、700、800、900 で、小さい順の 5 番目は 600 ms（p50）、9 番目は 900 ms（p95。(95 × 9 + 99) / 100 = 9）。除外した発話の数 1 を別の欄に書く（derived、D の数え方の 10。除外を遅れ 0 と読まない） | 境界、陰性 | `an_utterance_no_segment_touched_is_left_out_and_counted_apart` |
| T-RP-66-063 | 060 の発話の列と、A だけの検出の時刻の列（発話 k が 100k + 50 ms 後。割れ無し）を同じ関数に渡す | 比べる相手は同じ数え方の A だけの値 | 2 つの列 | R8v の列は p50 600、p95 1000。A だけの列は 150、250、…、1050 の 10 個で p50 5 番目の 550、p95 10 番目の 1050 を並べて出す（derived、D の数え方の 10。同じ nearest-rank） | 陽性 | `the_a_only_delay_is_counted_the_same_way_and_written_beside` |
| T-RP-66-029 | 001 の ja で、4 番目の run の前の load を `1.99 0.50 0.50 1/300 1000` にする | load の上側の境界の手前 | `Ok` | `Ok`（specified、6 の RP-66 の「RP-32 と同じ」と RP-32 の「2 未満」。199 < 200） | 境界、陽性 | `a_load_of_1_99_before_a_run_passes` |
| T-RP-66-030 | 029 の load を `2.00 0.50 0.50 1/300 1000` にする | 境界の値 | `Err` | `Err` で 4 番目の run を名指し（specified、RP-32。200 は 2 未満でない） | 境界、陰性 | `a_load_of_2_00_before_a_run_is_rejected` |
| T-RP-66-031 | 001 の ja で、先頭の 1 対だけ（main、R8v の 2 本） | 対が 1 つ | `Err` | `Err` で対の数が 2 でないことを名指し（specified、6 の RP-66 の「言語ごとに 2 対」） | 境界、陰性 | `one_pair_per_language_is_rejected` |
| T-RP-66-032 | ja。合わせた main の `d` の p50 が 804、805、925、926 ms の 4 入力 | 記録の 865 ms の ±60 ms の帯の両端 | 805 と 925 は `Ok`、804 と 926 は `Err` | 805（865 − 60）と 925（865 + 60）は帯の内で `Ok`、804 と 926 は `Err` で main の `d` の p50 を名指し（specified、D 3.17 の数え方の 4 の「差の絶対値 ≤ 60」、6 の RP-66 の陽性欄）。落ちた入力は assert の message に出す | 境界 | `main_d_p50_outside_the_recorded_band_for_ja_is_rejected` |
| T-RP-66-033 | en。合わせた main の `d` の p50 が 396、397、517、518 ms の 4 入力 | 記録の 457 ms の ±60 ms の帯の両端 | 397 と 517 は `Ok`、396 と 518 は `Err` | 397（457 − 60）と 517（457 + 60）は帯の内で `Ok`、396 と 518 は `Err` で main の `d` の p50 を名指し（specified、同上） | 境界 | `main_d_p50_outside_the_recorded_band_for_en_is_rejected` |
| T-RP-66-034 | 実物の記録：`docs/results/e2e/e2e1/e2e1.jsonl` と `docs/results/e2e/e2e2/e2e2.jsonl` を main の run として渡す | 実物の main の `d` | `Ok` | 有効な標本（ja n 100、en n 75）の nearest-rank で、ja の main の `d` の p50 は 865.5 ms、en は 456.6 ms で、どちらも帯の内側（805〜925、397〜517）で、D の ja 865 ms、en 457 ms と丸めて合う（pseudo、jq。`select(.t_final_us != null)` で `d ≥ 120`。2026-10-15 に打った）。D 3.17 の冒頭の表の 881 と 448 は訳の取れた標本の値で、この行には使わない（D 3.17 の数え方の 1） | 陽性 | `the_recorded_main_run_is_inside_the_band` |
| T-RP-66-035 | 同じ記録を R8v の run として渡す（`--endpoint silence` の run が返す形） | 早い規則が働かない run | `Err` | `Err` で 4 つの限度を名指し。有効な標本で、ja は p50 865.5 ms > 500 と p95 1341.4 ms > 1050（n 100 の 50、95 番目）、en は p50 456.6 ms > 300 と p95 706.3 ms > 520（n 75 の 38、72 番目）（pseudo、jq）。判定の道具が実物の遅れを「超える」と読む確かめ | 陰性 | `the_recorded_main_run_does_not_meet_the_d_limits` |
| T-RP-66-036 | R8v の run の `hayamimi` の log。`asr.endpoint.decided` が 3 件で、`rule` は `silence`、`max_speech`、`flush` | `early` が 0 件 | `Err` | `Err` で `rule` が `early` の行が無いことを名指し（specified、6 の RP-66 の陽性欄「`early` の行が 1 件以上」）。`silence`、`max_speech`、`flush` は数えない | 陰性 | `an_r8v_log_without_an_early_decision_is_rejected` |
| T-RP-66-037 | 036 の log に `rule` が `early` で `silence_ms` が 120 の行を 1 件足す | `early` が 1 件 | `Ok` | `Ok`（specified、同上。1 件が下限の等号側）。`early` の行は `id`、`rule`、`silence_ms` を持つ（3.17 の 6） | 境界、陽性 | `one_early_decision_in_the_log_is_enough` |
| T-RP-66-038 | 実走：`docs/results/r8v/` の対の番号 `<N>` ごとの `main<N>.e2e1.jsonl`、`r8v<N>.e2e1.jsonl`、`main<N>.e2e2.jsonl`、`r8v<N>.e2e2.jsonl` と `load<N>.txt`（D 3.17 の数え方の 6）、`hayamimi` の log | 実走 | 実走 | 001 の形で `Ok`（specified、6 の RP-66）。R8v の手番 3 の記録が置かれるまでは走らせない。ja の 1050 ms を置き直す席が終わるまで、手番 3 の run は打たない（D 3.17 の数え方の 5） | 陽性 | `rp66_early_end_shortens_the_wait_without_splitting_utterances` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 設計が決めた物（D 3.17 の数え方の 1〜10）は、行に反映した。残るのは次の物である。
- 仕分けの優先。割れの前の文が参照の文の終わりに合い、かつ後ろの文が参照の残りの無い所に付いた雑音である形（たとえば J5 の 17 字目で終わる割れ）は、本物の文の切れ目と尾の雑音の両方に当たる。数え方の 8 は優先を書かない。040〜055 の fixture はこの形を使っていない。
- 文の途中の切断の率の上限 U と、数え方の 9 の一致の上限は、評議が手番 1b の後に固定する。057 と 058 は U の値が決まったら数を書く。
- 遅れの数え方（060〜063）を置く file。D の 6 の RP-66 は `rp66.rs` を指すが、probe の関数（`r8v_probe.rs`）に在る数え方かは設計に無い。
- ja の p95 の限度 1050 ms を置き直したら、002〜005 の境界の値をこの席が書き直す（D の数え方の 5）。それまで 1050 で書いてある。
- 011 の根拠（main の run の `d` の最小が ja 428 ms、en 361 ms）は 3.17 の記録の値で、この席は打ち直していない。010 は記録の有効の数が 0.12 秒と 0.30 秒で一致すること（jq）だけを打ち直した。

### 4.65 RP-67（主指標 `L` の p95 を 1000 ms 以下にする）

**未実装。** この節の行はどれも、test の file も関数も repo に無い。実装の席が足す。
test：`native/hayamimi-eval/tests/rp67.rs`。受入 test の名前は `rp67_speech_end_to_translation_p95_is_within_one_second`。実走の行（T-RP-67-015）だけが `#[ignore]` を持ち、`docs/results/rp67/` の `e2e1.jsonl` と `e2e2.jsonl` を読む。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp67`。実走の行はこれに `-- --ignored` を足す。
関数の名前と引数の形は実装の席が決める。行は「言語ごとの run の標本の列を渡す」形で書く。

**数え方**（3.1 と D 3.17 の数え方の 3 のとおり）：`L = t_tr − t_end`。n は有効（`final` が在り `d ≥ min_silence`、RP-67 も `min_silence` は 0.12 秒。評議が GO）で訳の取れた標本の数。test は 0.30 秒でも数えて並べて記録する。p95 は nearest-rank で順位は (95 × n + 99) / 100 の整数除算。したがって 1000 ms を超えてよい標本の数は `n − 順位` である。

| n | 順位 | 超えてよい数 |
|---|---|---|
| 92 | 88 | 4 |
| 93（E2E-1 の記録の n） | 89 | 4 |
| 94 | 90 | 4 |
| 95 | 91 | 4 |
| 96 | 92 | 4 |
| 69 | 66 | 3 |
| 70（E2E-2 の記録の n） | 67 | 3 |
| 71 | 68 | 3 |

**fixture F67**：n 標本、全部 `d` 500 ms、`L_dec` 150 ms、`L_tr` 250 ms（`L` は 900 ms）。「k 件を超える」は、`L` の大きい方から k 件の `L_tr` を足して `L` を 1001 ms にする。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-67-001 | F67、n = 93（ja）と n = 70（en） | 全標本が 900 ms | `Ok` | 両方 `Ok`（specified、6 の RP-67 の「全標本の `L` を 900 ms に置いた fixture で通る」）。出力に `d` 500 ms、復号 150 ms、訳 250 ms、`L` 900 ms の p50 と p95 が並ぶ（specified、「同じ結果に並べて書く」） | 陽性 | `all_samples_at_900_ms_pass_and_the_columns_are_written` |
| T-RP-67-002 | F67、n = 93 で全標本の `L` を 1000 ms にする | 限度の値 | `Ok` | `Ok`（specified、6 の RP-67 の「1000 ms 以下」。等号側） | 境界、陽性 | `l_of_exactly_1000_ms_passes` |
| T-RP-67-003 | F67、n = 93 で 4 件を 1001 ms 以上にする | 超えてよい数ちょうど（ja） | `Ok` | `Ok`。89 番目は 900 ms（derived、(95 × 93 + 99) / 100 = 89。n − 89 = 4 件まで超えてよい） | 境界、陽性 | `four_over_in_93_samples_still_pass` |
| T-RP-67-004 | F67、n = 93 で 5 件を 1001 ms 以上にする | 超えてよい数より 1 件多い（ja） | `Err` | `Err` で `L` の p95 を名指し。89 番目が 1001 ms 以上（derived、同上。D の「許す数より 1 件多く入れた fixture で落ちる」） | 境界、陰性 | `five_over_in_93_samples_are_rejected` |
| T-RP-67-005 | F67、n = 70 で 3 件を 1001 ms 以上にする | 超えてよい数ちょうど（en） | `Ok` | `Ok`。67 番目は 900 ms（derived、(95 × 70 + 99) / 100 = 67。n − 67 = 3） | 境界、陽性 | `three_over_in_70_samples_still_pass` |
| T-RP-67-006 | F67、n = 70 で 4 件を 1001 ms 以上にする | 超えてよい数より 1 件多い（en） | `Err` | `Err` で `L` の p95 を名指し。67 番目が 1001 ms 以上（derived、同上） | 境界、陰性 | `four_over_in_70_samples_are_rejected` |
| T-RP-67-007 | F67、n が 92、94、95、96（ja の側）と 69、71（en の側）の各 n で、超える件数が表の数ちょうどの入力と 1 件多い入力 | n が動くと順位が動く | 表の数は `Ok`、1 件多いと `Err` | 表のとおり。n = 92 は 4 件で `Ok`、5 件で `Err`。n = 69 と 71 は 3 件で `Ok`、4 件で `Err`（derived、順位の式を各 n に当てた。順位を 93 と 70 の値に固定した build はここで落ちる） | 境界 | `the_allowed_count_follows_n` |
| T-RP-67-008 | main の記録の形：n = 70 で、1000 ms を超える標本が 32 件、上位 3 件が 4681、1658、1603 ms、残りの 29 件が 1001 ms 以上 | en の記録の形 | `Err` | `Err` で `L` の p95 を名指し。67 番目が 1001 ms 以上（derived、H69。上位 3 件が枠を使い切り、残りの 29 件が超えているので落ちる） | 陰性 | `a_run_shaped_like_the_recorded_en_run_is_rejected` |
| T-RP-67-009 | 008 の 29 件を 1000 ms 以下にした n = 70 | 上位 3 件だけが超える | `Ok` | `Ok`（derived、H69 の「残りの 29 件をすべて 1000 ms 以下にする必要がある」）。008 の対照 | 境界、陽性 | `only_the_top_three_over_1000_ms_pass_in_70_samples` |
| T-RP-67-010 | n = 100 の有効のうち、訳の取れない標本が 7（`t_tr_us` が null）、訳の取れた 93 のうち 5 件が 1001 ms 以上、88 件が 900 ms | 訳の無い標本を `L` が小さいと読む | `Err` | `Err`。n は 93、89 番目が 1001 ms 以上。訳の無い 7 標本を `L` 0 として 100 標本に混ぜると 95 番目が 900 ms になり通ってしまう（derived、(95 × 93 + 99) / 100 = 89。実物の記録は ja が有効 100 のうち訳ありが 93） | 陰性 | `samples_without_a_translation_are_not_counted_as_fast` |
| T-RP-67-011 | 訳の取れた有効な標本が 0 件の run | 空 | `Err` | `Err` で標本が無いことを名指し（specified、E2E-1 の T-E2E-1-008 と同じ。0 件を「1000 ms 以下」と読まない） | 陰性、境界 | `a_run_with_no_translated_sample_is_not_a_pass` |
| T-RP-67-012 | ja は全標本 1001 ms、en は全標本 900 ms | ja だけが落ちる | `Err` | `Err` で ja（E2E-1）だけを名指し。en は名指さない（specified、6 の RP-67 の「それぞれの `L` の p95」） | 陰性 | `a_failing_ja_run_is_named_alone` |
| T-RP-67-013 | ja は全標本 900 ms、en は全標本 1001 ms | en だけが落ちる | `Err` | `Err` で en（E2E-2）だけを名指し。ja は名指さない（specified、同上） | 陰性 | `a_failing_en_run_is_named_alone` |
| T-RP-67-014 | 今の main の記録：`docs/results/e2e/e2e1/e2e1.jsonl` と `docs/results/e2e/e2e2/e2e2.jsonl`（git の中に在る。D 3.17 の数え方の 6） | 実物の記録 | 両方 `Err` | ja は n 93 の `L` の p95 が 1871.7 ms（89 番目）、en は n 70 の p95 が 1303.2 ms（67 番目）で、どちらも `Err`。`L` が 1000 ms 以下の標本は ja が 6、en が 38（pseudo、jq。`select(.t_final_us != null and .t_tr_us != null)` と `d ≥ 300`、nearest-rank。2026-10-15 に打った）。出力に `d` の p50 881.5、p95 1355.1 ms（ja）、443.6、671.9 ms（en）も並ぶ | 陽性、陰性 | `the_recorded_main_runs_are_over_one_second` |
| T-RP-67-016 | **S2**：main の記録（014 の 2 本）。陰性対照として、ja の 1 標本の `d` を 250 ms にした main の run | main の run で 0.12 秒と 0.30 秒の有効の数 | 一致なら `Ok`、差が出たら `Err` | 記録は ja が 100 と 100、en が 75 と 75 で、0.12〜0.30 秒に入る標本は 0 件で `Ok`（pseudo、jq。`select(.t_final_us != null and d >= 120)` と `>= 300` を数えた。評議の打ち直しと同じ値。2026-10-15 に打った）。250 ms の標本を入れた run は 0.12 秒で 100、0.30 秒で 99 になり、`Err` で main の run の有効の数が合わないことを名指す（評議の S2。差が出たら落とす） | 陽性、陰性、境界 | `main_runs_count_the_same_valid_samples_at_120_and_300_ms_for_the_l_verdict` |
| T-RP-67-017 | **S3**：n 93 の run。88 標本は `d` 500 ms で `L` 900 ms。4 標本は `d` 500 ms で `L` 1001 ms。1 標本は `d` 250 ms で `L` 1500 ms（訳が取れている） | 0.12 秒でだけ有効な標本が 1 つ。数え方で判定が割れる | `Err` と並べた記録 | 0.12 秒の判定は n 93 で 5 件が 1000 ms 超、89 番目が 1001 ms で `Err`。0.30 秒で数えると n 92、4 件が超えて 88 番目が 900 ms になり通る。出力に 0.12 秒でだけ有効な標本の数 1 とその `L` 1500 ms を別に書き、`L` の p95 を 0.12 秒の値 1001 ms と 0.30 秒の値 900 ms の両方で並べる（derived、(95 × 93 + 99) / 100 = 89 と (95 × 92 + 99) / 100 = 88。評議の S3。0.30 秒で数える build はここで合格してしまうので落ちる） | 陰性、境界 | `a_slow_sample_valid_only_at_120_ms_counts_and_both_p95_values_are_written` |
| T-RP-67-018 | **S4**：n 93 の訳の取れた標本（全部 `L` 900 ms）に、訳の取れない有効な標本 7（`t_tr_us` が null）を足した run | 訳の取れない有効な標本 | `Ok` と数 | `Ok`。出力に訳の取れない有効な標本の数 7 が別の欄で出る。7 標本は n に入らない（010 と同じ）（derived、評議の S4。実物の記録は ja が 7、en が 5。jq の `select(.t_final_us != null and .t_tr_us == null)`） | 陽性 | `valid_samples_without_a_translation_are_counted_apart` |
| T-RP-67-015 | 実走：`docs/results/rp67/` の `e2e1.jsonl`（23 発話 × 5 回）と `e2e2.jsonl`（15 発話 × 5 回） | 実走 | 実走 | 両言語で `L` の p95 ≤ 1000 ms で `Ok`、`d`、復号、訳の p50 と p95 も出す（specified、6 の RP-67）。ja は訳の p95 の下限があるので R8v だけでは届かない（H69）。記録が置かれるまでは走らせない | 陽性 | `rp67_speech_end_to_translation_p95_is_within_one_second` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- RP-67 の有効の線は 0.12 秒で、評議が GO を出した（S2、S3、S4 は 016〜018）。D の 6 の RP-67 の行（`rust_port.ja.md:978`）にもこの線が書かれている。RP-67 の `L` は `t_tr − t_end` で取り、発話の終わりの遅れ（最後の final）の数え方は使わない。
- RP-67 の run が 1 回か複数回か（D は「5 回」の枠だけを書き、交互の対にしない）。この節は 1 つの run の列を渡す形にした。
- 015 の `docs/results/rp67/` の記録を、誰がどの手番で置くか。

### 4.66 RP-68（流れの途中の `flush` の再現と、R8v と `finish` の後の守り）

**未実装。** この節の行はどれも、test の file も関数も repo に無い。実装の席が足す。
test：`native/hayamimi-asr/tests/rp68.rs`。受入 test の名前は `rp68_flush_inside_speech_aborts_and_the_early_end_never_flushes`。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-asr --test rp68`。
関数の名前と引数の形は実装の席が決める。ただし case 単位の test の名前は、各行の「関数の末尾」と同じにする。

**形**（D 6 の RP-68 のとおり）：親の test が、自分の test の binary（`std::env::current_exe`）を子 process として `--ignored --exact <子の test の名前>` で起こし、子の終わり方（終了の status と stderr）だけを見る。行の「関数の末尾」は親の test の名前で、子の名前は親の名前に `_child` を付けた物にする。子は `#[ignore]` を持つ。

**音声の形**（窓は 512 sample = 32 ms、16 kHz）：

- **V68**：`testdata/lat-ja.wav` の 1 発話目の声の区間（manifest の `start_sample` から `end_sample`）。
- **無音 S4**：4 窓 = 2048 sample = 0.128 秒。**無音 S6**：6 窓 = 3072 sample = 0.192 秒。D の「0.12〜0.2 秒の無音」の両端に最も近い窓の数である。
- **長い無音 L12**：12 窓 = 6144 sample = 0.384 秒。D の「0.30 秒以上の無音」を満たす。`min_silence` 0.30 秒 = 4800 sample を超え、手番 1 の probe の `Invalid n: -2752`（2048 − 4800）と同じ向きに足りなくなる長さである。

**期待の読み方**（評議の条件）：

- 異常の判定は「終了の status が 0 でない」か「stderr に `Invalid n` が出る」のどちらかが当たること。abort とは断定しない。`circular-buffer.cc` の `Get` と `Pop` は `Invalid n` を log に出して空のまま戻るだけなので、process が落ちるとは断定しない。status が無い終わり方（signal で殺された）も「0 でない」に数える。
- 正常の判定は、2 つともそろうこと。status が 0、かつ stderr に `Invalid n` が無い。
- 各行は「今の main」と「直した後」を分ける。直しは、`Engine::finish` が `flush` と segment の取り出しの後に A と B を作り直して offset を進める小さな commit で、R8v とは別に入れる。順は評議の判定どおり、まず RP-68 の test だけを今の main に当て、子 3 が落ちたら直す。子 3 の「今の main」の欄は走らせて決まるので NOT VERIFIED と書く。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle）。今の main ／ 直した後 | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-68-001 | **子 1（再現）**：sherpa-onnx の `VoiceActivityDetector` を直に開き、V68 → S4 → `flush` → L12 を 512 sample ずつ食わせる | 無音の下端 | 異常 | 今の main ／ 直した後 とも異常（status が 0 でない、または stderr に `Invalid n`）。直しは `Engine::finish` だけを変えるので、`VoiceActivityDetector` を直に叩くこの子は変わらない（specified、D 3.17 の方針 4 の機構と RP-68。評議の条件 1） | 陽性、境界 | `a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_short_gap` |
| T-RP-68-002 | 001 の S4 を S6 にする | 無音の上端 | 異常 | 001 と同じ（specified、同上） | 陽性、境界 | `a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_long_gap` |
| T-RP-68-003 | 子 1 の対照：V68 → S6 → L12。`flush` を打たない | `flush` が無い | 正常 | 今の main ／ 直した後 とも正常（status 0、`Invalid n` 無し）（specified、D 3.17 の方針 4。機構は「流れの途中の `flush`」にある）。001 と 002 の陰性の対照 | 陰性 | `without_the_flush_the_same_audio_ends_cleanly` |
| T-RP-68-004 | **子 2（守り、R8v の形）**：同じ音声を `hayamimi-asr` の `Vad`（A と B）と確定の線の規則に通す。V68 → S6 → L12 | R8v の早い終わり | 正常、final 1 件 | 今の main：2 本目の detector と確定の線の規則が無く、この子は書けない（R8v の手番 2 まで走らせない）。直した後：status 0、stderr に `Invalid n` が無く、`final` が 1 件で、その `t1_s` が確定の線の sample である（specified、D 6 の RP-68 の子 2。評議の条件 2） | 陽性 | `the_early_end_never_flushes_the_detector` |
| T-RP-68-005 | 004 を、確定の時に A へ `flush` を打つ build で流す | 守りが破られた build | 異常 | 直した後の build に `flush` を足した物は、異常（status が 0 でない、または stderr に `Invalid n`）で 004 が落ちる（specified、D 6 の RP-68 の陰性欄）。今の main には 004 が無いので走らせない | 陰性 | `a_build_that_flushes_at_the_early_end_fails_the_guard` |
| T-RP-68-006 | **子 3（守り、`finish` の後）**：V68 → S4 → `Engine::finish` → L12 を feed する | 声の後の無音の後で finish。下端 | 正常 | 今の main：落ちうる（H75。NOT VERIFIED。落ちる時は status が 0 でない、または stderr に `Invalid n`）。直した後：status 0 で stderr に `Invalid n` が無い（specified、D 6 の RP-68 の子 3 と H75。評議の条件 2、3。声の最中に finish する形ではなく、声の後の無音を挟む形にする） | 陽性、境界 | `finish_after_a_short_silence_then_a_long_silence_ends_cleanly` |
| T-RP-68-007 | 006 の S4 を S6 にする | 上端 | 正常 | 006 と同じ。今の main：落ちうる（NOT VERIFIED）。直した後：正常（specified、同上） | 陽性、境界 | `finish_after_a_long_gap_then_a_long_silence_ends_cleanly` |
| T-RP-68-008 | **子 3 の対照（評議の条件 3）**：V68 の途中（声の最中。`end_sample` より前の 0.5 秒目）で `Engine::finish` → L12 を feed する | 声の最中に finish する形 | 正常 | 今の main ／ 直した後 とも正常（status 0、`Invalid n` 無し）（specified、評議の条件 3。NOT VERIFIED、走らせて確かめる）。欠陥があってもこの形は通るので、006 と 007 の形でなければ守りを測れない | 陰性、境界 | `finish_inside_speech_does_not_expose_the_defect` |
| T-RP-68-009 | 子 3 の続き：006 の後に、V68 をもう 1 回 feed する | `finish` の後の同じ音声 | 正常 | 今の main：落ちうる（NOT VERIFIED）。直した後：status 0 で stderr に `Invalid n` が無い（specified、D 6 の RP-68 の子 3「続けて同じ音声をもう 1 回食わせても」） | 陽性 | `a_second_utterance_after_finish_ends_cleanly` |
| T-RP-68-010 | **harness の対照**：偽の子 2 つ。(1) `exit(3)` で終わる子。(2) 自分に SIGABRT を打つ子（status が無い） | 終わり方の読み | どちらも異常 | 今の main ／ 直した後 とも、どちらも異常と読む。(2) は終了の code が無いので「0 と読んで通す」読み方を落とす（specified、評議の条件 1 の「status が 0 でない」。implicit） | 陽性、境界 | `the_harness_reads_a_nonzero_and_a_signalled_child_as_abnormal` |
| T-RP-68-011 | 偽の子：stderr に `Invalid n: -1. size: 8` を出して status 0 で終わる | stderr だけの異常 | 異常 | 今の main ／ 直した後 とも異常と読む（specified、評議の条件 1。process が落ちずに log だけ出す形） | 陽性 | `the_harness_reads_invalid_n_on_stderr_alone_as_abnormal` |
| T-RP-68-012 | 偽の子：何も出さず status 0 で終わる | 正常な終わり | 正常 | 今の main ／ 直した後 とも正常と読む（001 から 011 の陰性の対照） | 陰性 | `the_harness_reads_a_silent_clean_child_as_normal` |
| T-RP-68-013 | 子の test の名前を存在しない名前にして起こす（`--exact` が 0 本の test に当たる） | 子が走っていない | `Err` | 今の main ／ 直した後 とも `Err` で 0 本しか走らなかったことを名指す。`--exact` の名前違いは status 0 で終わるので、そのまま正常と読ませない（derived。cargo の test binary は当たる test が無くても status 0 で終わる） | 陰性、境界 | `a_child_name_that_runs_no_test_is_rejected` |
| T-RP-68-014 | 子 1 の代わりに、正常な偽の子（012 の形）を渡す | 再現の前提が変わった | `Err` | 今の main ／ 直した後 とも `Err` で「子 1 が異常にならない。再現の前提が変わった」を言う（specified、D 6 の RP-68 の陰性欄「子 1 が status 0 で終わる時はこの test が落ちて再現の前提が変わったことを言う」） | 陰性 | `a_clean_reproduction_child_is_reported_as_a_changed_premise` |
| T-RP-68-015 | 受入：001、002、004、006、007、009 の子を順に起こす親の test | 全体 | 通る | 今の main：001 と 002 は異常で 003 は正常。006 と 007 と 009 は走らせた結果で決まる（NOT VERIFIED）。004 は走らせない。直した後（R8v の手番 2 の後）：全部がそろって通る（specified、D 6 の RP-68） | 陽性 | `rp68_flush_inside_speech_aborts_and_the_early_end_never_flushes` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- `testdata/lat-ja.wav` は `.gitignore` の `testdata/` に入っていて、main の tree（`/mnt/c/Users/user01/source/hayamimi/testdata/`）にだけ在る。worktree の中には無い。子が音声を読む置き場（絶対 path か、`tests/golden/ja/` の追跡済みの wav か）は設計に無い。
- V68 の切り方。D は「1 発話の声の区間」とだけ書く。この節は lat-ja の 1 発話目と書いたが、manifest の file 名と欄は確かめていない。
- 直した後の 009 で、2 回目の発話が `final` を出すか、その `t1_s` の値。D は「status 0 で終わる」とだけ書く。offset を進める量は設計の席が決める。
- 008 の「声の最中」の位置（0.5 秒目）は、この席が置いた。評議の条件 3 は形だけを定めた。

### 4.67 RP-69（`final` が最後の速報を閉じるだけなら、速報の訳を使い回す）

**実装済み。** この節の行は `native/hayamimi-relay/tests/rp69.rs` に在る。
test：`native/hayamimi-relay/tests/rp69.rs`。受入 test の名前は `rp69_a_final_that_only_closes_the_last_partial_reuses_its_translation`。30 本のうち実走の行は無く、`--ignored` を付けずに通る。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp69`。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない（この file の形）。引数の形は実装の席が決める。
設計は D 3.18 の「部分訳の使い回し」と D 6 の RP-69 である。

**F69**（偽の認識器と偽の llama-server。`tests/support/events.rs` の `FakeRecognizer`、`run_relay_partials`、`FakeLlm` と同じ形）：

- **流れ**：先頭に確定文 L0（`mic-1`、`japanese_text(9)`）を流し、L0 の確定の訳が出るまで待つ。対象の発話は次の `mic-2` で、速報は `id` を持たないので中継が L0 の `final` の次の `id` として `mic-2` を読む（`partial.rs` の `Next::id`）。
- **原文**：`X` は `japanese_text(1)` から末尾の `。` を除いた 4 字。`X。` は `japanese_text(1)`。`Y` は `japanese_text(2)` から `。` を除いた 4 字。
- **偽の server**：T の依頼に 300 ms 後に答える。E は 100 ms 後。T の答えは `english_of` の字写しなので、`X` の答えは `.` を持たない 4 字 `aiue` の形で、`contains_eos` に当たらない。server は依頼ごとに種類（T か E）、依頼の文の原文、着いた時刻、打ち切られたか、同時に走っている数を記録する。
- **時刻**：`Sent::send_through` が押す時刻から、収集役 `collect_from` が event を受けた時刻までを測る。
- **数え方**：「T の依頼の数」は、種類が T で、原文が指した文字列に等しい依頼の数。`X` と `X。` は別の文字列として数える。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-69-001 | F69。速報 `X`（mic-2）を送り、その速報の訳（`of` が `partial`）が出るのを待つ。そのあとに `final` `X。`（mic-2）を送る | (a)。鍵が合う（音源、`id`、`X` + `。` 1 字）。速報の答えが出ている（`state` が `done`） | `final` を押してから訳が出るまで 50 ms 以内 | 原文が `X。` の T の依頼は 0 本。原文が `X` の T の依頼は 1 本（速報の分だけ）。`final` の訳 1 件が 50 ms 以内に出て、`of` が `final`、`stage` が `t`、`settled` が偽、`src_sha256` が `X。` の sha256、`text` が `aiue`。そのあと E が 1 本、`stage` が `e`、`settled` が真の訳が出る（specified、D 6 の RP-69 (a)。50 ms は T の 300 ms より十分に小さく、T を頼み直せば 300 ms 以上かかる） | 陽性 | `a_done_partial_answer_is_reused_without_asking_t_again` |
| T-RP-69-002 | 001 と同じ。`translate.draft.reused` と `translate.line.ok` の log を読む | log の項目 | 1 件 | `translate.draft.reused`（info）がちょうど 1 件。項目は `id`（`mic-2`）、`source`（`mic`）、`match`（`closing_mark`）、`state`（`done`）、`waited_ms`、`verdict`（`ok`）の 6 つで、`X`、`X。`、訳の文字列を載せない。その行の `translate.line.ok` の `t_ms` が `waited_ms` と同じ値（specified、D 3.18 の「5. log」） | 陽性 | `the_reuse_is_logged_once_with_its_six_items_and_no_text` |
| T-RP-69-003 | 001 の `final` を `X`（`。` を足さない同じ文字列）にする | 鍵の文字列が等しい。足す文字が 0 字 | 再利用 | T の依頼は原文 `X` の 1 本だけで、原文が `X` の `final` のために増えない。`translate.draft.reused` が 1 件で `match` が `same`。`src_sha256` が `X` の sha256（specified、D 3.18 の 1「同じか、後ろに `。` を 1 字足した物と同じ」。今の記録では「同じ」の行は 0 だが、鍵の定義には入っている） | 境界、陽性 | `a_final_with_the_same_text_reuses_the_answer_and_logs_same` |
| T-RP-69-004 | 001 の `final` を `X？` にする | 足す文字が `？`。問いに変わる | 再利用しない | 原文が `X？` の T の依頼が 1 本出る。`translate.draft.reused` が 0 件。速報の依頼が走っていれば打ち切られ、その `dropped` が 1 になる（今のとおり）（specified、D 3.18 の 1「`？` と `！` を足した形は合わない」と 6「変えない物」） | 陰性 | `a_final_that_adds_a_question_mark_asks_t_again` |
| T-RP-69-005 | 001 の `final` を `X！` にする | 足す文字が `！` | 再利用しない | 004 と同じ（specified、同上） | 陰性 | `a_final_that_adds_an_exclamation_mark_asks_t_again` |
| T-RP-69-006 | 001 の `final` を次の 3 つにして 1 つずつ流す。`X。。`、`X、`、`X.`（ASCII の `.`） | 足す文字が `。` 1 字ではない（2 字、別の句読点、別の文字種の終止符） | 再利用しない | 3 つとも、原文がその `final` の文字列の T の依頼が 1 本出て、`translate.draft.reused` が 0 件。落ちた入力を assert の message に出す（specified、D 3.18 の 1「`。` を 1 字足した物」。`。` が 2 字でも、`.` でも、同じ型の別の文字でも合わない） | 境界、陰性 | `a_final_that_adds_anything_but_one_closing_mark_asks_t_again` |
| T-RP-69-007 | 001 の `final` を `Y。`（別の文字列）にする | 文字列が違う。ITN で書き替わった形もここに入る | 再利用しない | 原文が `Y。` の T の依頼が 1 本出て、`translate.draft.reused` が 0 件。速報の依頼は止まる（specified、D 3.18 の 2「合わなければ今のとおり止める」と H80 の発話 8、13） | 陰性 | `a_final_with_other_text_asks_t_again` |
| T-RP-69-008 | L0 の後に速報 `X` を送る（読む `id` は `mic-2`）。`final` は `X。` で `id` を `mic-4` にする | `id` が違う | 再利用しない | 原文が `X。` の T の依頼が 1 本出て、`translate.draft.reused` が 0 件（specified、D 6 の RP-69 の陽性欄「`id` の違う `final`」） | 陰性 | `a_final_with_another_id_asks_t_again` |
| T-RP-69-009 | L0（mic）の後に速報 `X` を送る。`final` は `X。`、`id` は `mic-2` のまま、`source` を `loopback` にする | 音源が違う | 再利用しない | 原文が `X。` の T の依頼が 1 本出て、`translate.draft.reused` が 0 件（specified、D 3.18 の 1「音源、`id`、文字列の 3 つが合う時だけ」。3 つのうち 2 つが合っても使い回さない） | 陰性 | `a_final_from_another_source_asks_t_again` |
| T-RP-69-010 | 速報を 2 つ送る。先に `X`、後に `Y`（どちらも訳が出るまで待つ）。`final` は `X。` | 鍵に合うのは最後ではない速報 | 再利用しない | 原文が `X。` の T の依頼が 1 本出て、`translate.draft.reused` が 0 件。最後の速報 `Y` の文字列と比べる（specified、D 3.18 の「同じ発話の最後の速報」） | 境界、陰性 | `only_the_last_partial_is_compared_with_the_final` |
| T-RP-69-011 | 010 の順を逆にする。先に `Y`、後に `X`。`final` は `X。` | 鍵に合うのは最後の速報 | 再利用 | 原文が `X。` の T の依頼は 0 本で、`translate.draft.reused` が 1 件。010 の陽性の対照 | 境界、陽性 | `the_last_partial_is_the_one_that_is_reused` |
| T-RP-69-012 | L0 の後、速報を送らずに `final` `X。` を送る | 速報が無い。今の挙動 | 再利用しない | 原文が `X。` の T の依頼が 1 本出て、`translate.draft.reused` が 0 件（specified、D 3.18 の 3「どちらも無ければ今のとおり slot を取って T を頼む」）。001 から 011 の読み方の基準 | 陰性 | `a_final_without_a_partial_asks_t_as_before` |
| T-RP-69-013 | F69。速報 `X` を送り、偽の server に `X` の T の依頼が着いたのを見てから 100 ms 後に `final` `X。` を送る | (b)。速報の依頼が走っている間に `final` が来る（`state` が `running`） | 待ちは 200 ms を中心に [100, 400] ms | 速報の依頼は打ち切られず（`dropped` が 0）、その発話の T の依頼は全部で 1 本（原文が `X` の 1 本、原文が `X。` の 0 本）。`final` の訳（`stage` が `t`）は、速報の答えが着いた時（依頼が着いてから約 300 ms）に出る。`translate.draft.reused` が 1 件で `state` が `running`、`waited_ms` が [100, 400]。その行の `translate.line.ok` の `t_ms` が `waited_ms` と同じ（specified、D 6 の RP-69 (b)。300 − 100 = 200 ms が残りで、下の端は押す時刻の揺れ、上の端は機械の揺れを見込んだ幅） | 陽性 | `a_running_partial_is_awaited_not_aborted` |
| T-RP-69-014 | 013 の `final` を押した直後（速報の答えが着く前）に、次の発話の速報（`Y`、`mic-3` と読まれる）を送る | (b) の続き。次の発話の速報が来る | 打ち切らない | 013 の速報の依頼は打ち切られず（`dropped` が 0）、013 の `final` の訳が出る。次の発話の速報の依頼は別に出る（specified、D 3.18 の 2「行に渡すと決めた依頼は、次の発話の速報が来ても止めない」） | 陽性 | `the_next_partial_does_not_abort_the_request_a_line_awaits` |
| T-RP-69-015 | L0 の後に、速報を持たない確定文 4 本（`japanese_text(11)` から `(14)`）を 20 ms 間隔で流す。偽の server はその 4 本の T の依頼を 600 ms 眠らせ、slot（`LLAMA_SLOTS` の 4）を埋める。4 本が走っている間に、対象の速報 `X`（`id` は直前の確定文の次）を送り、すぐ `final` `X。` を送る | 速報の依頼が worker にまだ取られていない（待ち） | 再利用ではない。lane が T を頼む | 原文が `X` の T の依頼は 0 本（待っていた速報は mailbox から外れ、server に着かない）。原文が `X。` の T の依頼が 1 本出る。`translate.draft.reused` は 0 件。行は訳される（specified、D 3.18 の 2「待っている速報の文字列が合う時は、それを mailbox から外し、lane が自分で T を頼む」） | 境界、陰性 | `a_queued_partial_is_dropped_and_the_lane_asks_t_itself` |
| T-RP-69-016 | 013 で、偽の server が原文 `X` の T の依頼に HTTP 500 を返す | 速報の依頼が失敗した。lane が待っている | lane が T を頼む | 原文が `X。` の T の依頼が 1 本出て、行の訳（`stage` が `t`）が出る。速報の失敗で行が訳されないまま終わらない（specified、D 3.18 の 3「失敗したか打ち切られた時は、待っていた lane が T を頼む」） | 陰性、境界 | `a_failed_partial_request_makes_the_lane_ask_t` |
| T-RP-69-017 | L0 の後、6 つの発話を 1 本の流れに続けて流す。各発話は速報 `X_i` を押して 30 ms 後に `final` `X_i。` を押し、すぐ次の発話に移る | 速報の依頼が slot を使い、lane が待つ。slot は 4 本 | slot を持ったまま待たない | 6 つすべての `final` の訳が 3 秒以内に出る（直列でも 6 × (300 + 100) = 2400 ms で、600 ms の余裕）。server が同時に受けた依頼の最大が 4 以下。発話ごとの T の依頼（原文が `X_i` と `X_i。` の合計）が 1 本以下（derived、D 3.18 の 3「slot を持ったまま待たないのは、待たれている速報の依頼も slot を 1 本使っており、slot 4 本の中で互いを待つ形になる」。lane が slot を持って待つ build は、4 本の lane が slot を取り、速報の依頼が slot を取れずに止まるので、3 秒を超えて落ちる） | 陰性 | `lanes_waiting_for_partials_do_not_hold_a_slot` |
| T-RP-69-018 | 001 で、偽の server が原文 `X` の T に「`aiue 7`」（原文に無い数 7）と答える | (c)。速報の T の答えが V に落ちる。`state` が `done` | `final` から E の依頼が着くまで 50 ms 以内 | 速報の訳は V で止まっている（`translate.partial.held`）。`final` `X。` の後、原文が `X。` の T の依頼は 0 本。E の依頼が 1 本、`final` を押してから 50 ms 以内に server へ着く。`final` の訳のうち `stage` が `t` の物は出ない。`translate.draft.reused` が 1 件で `verdict` が `held`。E の答えが V を通れば `stage` が `e` の訳が出る（specified、D 6 の RP-69 (c) と D 3.18 の 4「落ちれば先の訳を出さずにすぐ E を頼む」。T の頼み直しなら E の依頼は 300 ms 以上遅れる） | 陰性、境界 | `a_reused_answer_that_fails_v_sends_e_at_once` |
| T-RP-69-019 | 018 の形で、`final` `X。` を、速報の依頼が走っている間（着いて 100 ms 後）に押す | (c) の `state` が `running` | 速報の答えが着いてから E の依頼が着くまで 50 ms 以内 | 速報の依頼は打ち切られない。その答え（`aiue 7`）が着いた時刻から 50 ms 以内に E の依頼が 1 本着く。原文が `X。` の T の依頼は 0 本。`stage` が `t` の訳は出ない。`translate.draft.reused` が 1 件で `state` が `running`、`verdict` が `held`（specified、D 3.18 の 3 と 4 の合わせ。答えを待ってから V、落ちれば E をすぐ） | 陰性、境界 | `a_running_answer_that_fails_v_sends_e_as_soon_as_it_arrives` |
| T-RP-69-020 | 001 に、V の入力の原文を記録する差し込み口を付ける（口の名前は実装の席が置く）。速報の V と、使い回した答えの V の入力を分けて読む | (d)。V の原文 | `X。` | 使い回した答えを V に掛けた時の原文が `X。`（`final` の文字列）で、`X` ではない（specified、D 3.18 の 4「V の原文は `final` の文字列にする。速報で掛けた V の判定は使わない」）。速報の V の原文は `X` | 陽性 | `the_reused_answer_is_verified_against_the_final_text` |
| T-RP-69-021 | 020 の記録を、001 の形（T が通り E を頼む）と 018 の形（T が落ち E を頼む）の 2 つで読む | (e)。1 行の V の回数 | 多くて 2 回 | 原文が `X。` の V の入力は、001 の形で 2 回（使い回した T の答えと E の答え）、018 の形でも 2 回（落ちた T の答えと E の答え）。3 回以上にならない（specified、D 6 の RP-69 (e) と D 3.18 の 4） | 境界 | `a_reused_line_runs_v_at_most_twice` |
| T-RP-69-022 | 001 の E の依頼の prompt を読む | 使い回した T の答えの行き先 | E の `Draft` 欄 | E の prompt の `Draft` 欄の本文が、速報の T の答え `aiue` に等しい（derived。`tests/support/events.rs` の `read_prompt` が prompt から `Draft` 欄を読む。T を頼み直さなくても E が同じ下書きから始まる） | 陽性 | `e_is_asked_with_the_reused_answer_as_its_draft` |
| T-RP-69-023 | 001 の `final` の訳の event と、004（`X？`）の `final` の訳のうち `stage` が `t` の event の、key の集合を比べる | wire の形 | 同じ | 2 つの event の key の集合が等しく、`status` が `ok`、`settled` が偽、`of` が `final`。使い回しで wire に key が増えも減りもしない（pseudo、同じ test の中の使い回さない行。specified、D 3.18 の 6「wire の形」を変えない） | 陽性 | `a_reused_translation_has_the_same_wire_keys_as_a_fresh_one` |
| T-RP-69-024 | 既存の `native/hayamimi-relay/tests/r8d_partial.rs` の `the_final_of_the_utterance_drops_the_text_in_progress` | 速報と違う文字列の `final` | 通ったまま | 実装の後も通る。速報の依頼が走っている間に違う文字列の `final` が来ると、依頼は打ち切られる。新しい関数は足さず、この既存の test を回帰として数える（specified、D 3.18 の「数えた結果」の 3 行目と 6「文字列が合わない時の挙動」）。実装の席は、`partial.rs` の module の doc comment の「`final` は閉じた発話の速報を捨てる」を同じ commit で書き替える | 陽性 | `the_final_of_the_utterance_drops_the_text_in_progress` |
| T-RP-69-025 | 受入：001 から 003、013、018、020、021 の形を 1 本に通す | 全体 | 通る | (a) 001（T 0 本、50 ms 以内、`closing_mark`）、(b) 013（T 全部で 1 本、打ち切らない、`running`）、(c) 018（E を 50 ms 以内に頼む、先の訳を出さない）、(d) 020（V の原文が `X。`）、(e) 021（V は 1 行に 2 回以下）が、1 回の test の中ですべて成り立つ（specified、D 6 の RP-69）。陽性欄の `X？` と `id` の違う `final` は 004 と 008 が持つ | 陽性 | `rp69_a_final_that_only_closes_the_last_partial_reuses_its_translation` |
| T-RP-69-026 | `Mailbox` に日本語の速報 `TELASA で独占` と `TELASAで独占`（空白の有無が逆）と `TELASA  で 独占` を置き、答えが着いた後に、空白だけが違う日本語の `final` を閉じる | 日本語が元の行。空白だけが違う | 使い回す | `claim` が `Some`、`match` が `same`（specified、D 3.18 の 1「日本語が元の行は半角空白を取り除いて比べる」） | 陽性 | `a_japanese_final_that_differs_only_in_spaces_is_the_same_text` |
| T-RP-69-027 | 026 の速報に、空白の違いと `。` 1 字が重なる `final`（`TELASAで独占。`、`TELASA で独占。`、`TELASA で独占 。`）と、空白が同じで `。` だけ足した `final` | 日本語が元の行。空白の違いと句点が重なる | 使い回す | 4 つとも `claim` が `Some`、`match` が `closing_mark`（specified、同上） | 境界、陽性 | `a_japanese_final_that_adds_a_closing_mark_and_differs_in_spaces_closes_the_text` |
| T-RP-69-028 | 英語が元の行の速報 `ab c` に `a bc`、`buy a car` に `buy  a car` と `buyacar` の `final` | 英語が元の行。空白が語の切れ目 | 使い回さない | 3 つとも `claim` が `None`。同じ文字列の `final` は `same`（対照）（specified、同上「英語が元の行は取り除かない」） | 陰性 | `an_english_final_that_differs_in_spaces_is_another_text` |
| T-RP-69-029 | 026 の速報 `TELASA で独占` に、本文の違う `final`（`TELASAで配信`、`TELASA で配信。`）、別の句読点（`TELASAで独占？`）、`。` が 2 字（`TELASAで独占。。`）、`。` が末尾でない（`TELASAで独占。 まで`）、文字が足りない（`TELASAで独` ） | 日本語が元の行。空白の他が違う | 使い回さない | 6 つとも `claim` が `None`（specified、同上。空白を取り除いても、それ以外の違いは合わせない） | 陰性、境界 | `a_japanese_final_with_other_letters_is_another_text` |
| T-RP-69-030 | F69。速報 `X`（4 字）の訳が出た後に、`X` の途中に半角空白を 1 つ入れて `。` を足した `final`（`mic-2`）を送る | 全体の経路。日本語の音源の `final` | 使い回す | 原文がその `final` の文字列の T の依頼は 0 本、`X` の T の依頼は 1 本。`translate.draft.reused` が 1 件で `match` が `closing_mark`。行は settle する（specified、同上） | 陽性 | `a_final_with_a_space_and_a_closing_mark_reuses_the_answer` |

026 から 030 は実装済みで、`--ignored` を付けずに通る。026 から 029 は偽の認識器も server も使わず、`Mailbox` の `post`、`take`、`begin`、`close`、`finish`、`claim` を直に呼ぶ。陰性の build（空白を取り除かずに比べる build）で落ちる行は 026、027、030 である。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- V の入力の原文を記録する差し込み口（020、021）。D は「observer の `draft_verify_input` と V の入力で数える」と書くが、`draft_verify_input` が受けるのは訳の文字列で、原文は受けない。原文を読む口は設計に無い。実装の席が置き、口の名前を行へ戻す。
- 013 の `waited_ms` の幅 [100, 400] ms は、この席が置いた（中心は 300 − 100 = 200 ms）。D は 50 ms の外の幅を決めていない。
- 015 の「待ち」の作り方（slot を 4 本埋めてから速報を送る）はこの席が置いた。worker が取る順の仕組みは D に書かれていない。
- 016 で、速報の失敗の時に `translate.draft.reused` を出すか。D は「使い回した行ごとに 1 件」と書く。lane が T を頼んだ行は使い回した行ではないので、この席は 0 件と読んだが、行では見ていない。
- 017 の発話の間隔 30 ms と 6 発話は、この席が置いた。
- 020 の速報の V の原文（`X`）は、速報の経路が V を掛ける時の原文を指す。D 3.18 は速報の V の原文を書いていない。
- 速報が `final` の前に 2 回以上更新される時、`Mailbox` が最後の速報を残す仕組み。010 と 011 が外から見える結果だけを固定する。

### 4.68 RP-70（`。` を足しただけの文の訳を使い回してよい）

**未実装。** この節の行はどれも、test の file も関数も repo に無い。実装の席が足す。
test：`native/hayamimi-eval/tests/rp70.rs`。受入 test の名前は `rp70_translating_without_the_closing_mark_keeps_chrf_and_verify`。実走の行（T-RP-70-013）だけが `#[ignore]` を持ち、E2E の席が llama-server に打った 4 組の結果を読む。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp70`。実走の行はこれに `-- --ignored` を足す。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。引数の形は実装の席が決める。
設計は D 3.18 の「部分訳の使い回し」の品質の段落と D 6 の RP-70、H85 である。

**判定の関数**：`check_closing_mark(a, b)` は、同じ条件で打った 2 回ずつの結果を受ける。`a` は末尾が `。` のままの原文の組の `a1` と `a2`、`b` は末尾の `。` だけを除いた組の `b1` と `b2` である。1 つの結果は、`<ev> chrf` の corpus chrF（小数 6 桁まで）と、中継の stdout の `translate.line.held` のうち T と E の両方を止めた行の数（RP-42 の理由の欄が 2 つ在る行）と、訳の文末の記録である。chrF は 2 回を合わせた corpus chrF で比べる（D 6 の「2 回を合わせた」）。小数は 10 の -6 乗の整数に読んで比べる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-70-001 | `a` と `b` の chrF が同じ（a の 2 回を合わせた値と b の 2 回を合わせた値が 40.123456）。止めた行は `a` の合計 3、`b` の合計 3 | chrF の等号。止めた行の数の等号 | b ≥ a、b の止めた行 ≤ a の止めた行 | `Ok`（specified、D 6 の RP-70 の「以上」と「以下」。等号を通す） | 境界、陽性 | `equal_chrf_and_equal_held_rows_pass` |
| T-RP-70-002 | 001 の `b` の chrF を 40.113456 にする（a より 0.01 低い） | 設計の陰性 | b < a | `Err` で chrF を名指し。b の値と a の値を出す（specified、D 6 の RP-70 の陰性欄「b の chrF を a より 0.01 低く置いた fixture」） | 陰性 | `a_chrf_one_hundredth_below_a_is_rejected` |
| T-RP-70-003 | 001 の `b` の chrF を 40.123455 にする（最小の桁 1 つ下） | 境界の下の側 | b < a | `Err` で chrF を名指す（specified、同上。4.1 の「最小の桁 1 つ分だけ外れた値」） | 境界、陰性 | `a_chrf_one_micro_unit_below_a_is_rejected` |
| T-RP-70-004 | 001 の `b` の chrF を 40.123457 にする（最小の桁 1 つ上） | 境界の上の側 | b ≥ a | `Ok`（specified、同上） | 境界、陽性 | `a_chrf_one_micro_unit_above_a_passes` |
| T-RP-70-005 | 001 の `b` の止めた行を 4 にする（a は 3 のまま） | 設計の陰性 | b の止めた行 > a | `Err` で止めた行を名指し。4 と 3 を出す（specified、D 6 の RP-70 の陰性欄「b の止めた行を a より 1 行多く置いた fixture」） | 陰性、境界 | `one_more_held_row_in_b_is_rejected` |
| T-RP-70-006 | 001 の `b` の止めた行を 2 にする（a は 3） | 止めた行が b の方が少ない | b の止めた行 ≤ a | `Ok`（specified、同上） | 境界、陽性 | `fewer_held_rows_in_b_pass` |
| T-RP-70-007 | `a1` の chrF 40.00、`a2` 40.00 を合わせた値 40.00。`b1` の chrF 39.00、`b2` の chrF 42.00 を合わせた値 40.50（b1 は a1 より低く、b2 は a2 より高い） | 2 回を合わせた比べ。1 回ずつではない | b ≥ a（合わせた値） | `Ok`。1 回ずつ比べると b1 が a1 を下回って落ちるが、合わせた値は 40.50 ≥ 40.00 で通る（specified、D 6 の RP-70「2 回を合わせた corpus chrF」と H85。割れは 2 回の合わせで読む） | 境界、陽性 | `the_two_runs_are_pooled_before_the_comparison` |
| T-RP-70-008 | 007 の `b2` を 40.50 にして、合わせた値を 39.75 にする | 合わせた値が a を下回る。b2 は a2 より高い | b < a（合わせた値） | `Err` で chrF を名指し。b2 が a2 を上回っても、合わせた値が下なら落ちる（007 の陰性の対照） | 陰性 | `a_pooled_value_below_a_fails_even_if_one_run_is_above` |
| T-RP-70-009 | 001 に、b の訳の文末に `.`、`?`、`!` が無く、a の同じ行の訳には在る行を 5 つ足した入力（chrF と止めた行は 001 のまま） | 記録だけの欄 | 合否に入れない | `Ok`。出力にその行の数 5 が在る（specified、D 6 の RP-70「記録する（合否に入れない）」）。数を出す欄が無い出力は `Err` で欄を名指す | 陽性、陰性 | `rows_that_lose_their_final_mark_are_recorded_and_not_judged` |
| T-RP-70-010 | `a` の 2 回だけから作った入力（`a1` と `a2` を、b の位置にも置く） | a 同士の差 | 差を出す | `Ok`。a1 と a2 の chrF の差と止めた行の差の 2 つが出力に在る（specified、D 6 の RP-70 の陽性欄「a の 2 回の対から chrF の差と止めた行の差を出す」と H85）。2 つの値が出ない出力は `Err` | 陽性 | `the_difference_between_the_two_a_runs_is_reported` |
| T-RP-70-011 | 組の作りの確かめ。fixture の `a` の組 3 行と、`b` の組 3 行（原文の末尾の `。` だけが無く、`id` と参照訳は同じ） | 作りが正しい | 全行 | `Ok`。全行で `b` の原文 + `。` が `a` の原文に等しく、`id` と参照訳が等しい。`a` の行の数と `b` の行の数が等しい（specified、D 6 の RP-70「末尾の `。` だけを除いた組 b（行の id と参照訳は同じ）」） | 陽性 | `a_b_pair_set_is_the_a_set_without_the_final_closing_mark` |
| T-RP-70-012 | 011 の `b` の 1 行で、`。` に加えて文末の 1 字も除いた原文。もう 1 つの入力は、`a` の組に末尾が `。` でない行が混ざった組 | `。` だけの違いではない。`a` に対象でない行が混ざる | `Err` | 前者は `Err` でその `id` を名指す。後者も `Err` でその `id` を名指す（末尾が `。` の行だけを取る。specified、D 6 の RP-70）。空の組（`。` で終わる行が 0 行）も `Err` で、seed 0 か seed 1 のどちらの組かを名指す。0 行を「差 0 で一致」と読ませない | 陰性、境界 | `a_b_set_that_differs_by_more_than_the_mark_or_an_empty_set_is_rejected` |
| T-RP-70-013 | 実走：同じ llama-server（RP-71 の腕 N0）に、`a1`、`b1`、`a2`、`b2` の順で打った結果の dir と、中継の stdout を読む。seed 0 の組（`testdata/baseline/pairs/`）と seed 1 の組（`testdata/baseline/pairs_seed1/`）の ja の原文のうち、末尾が `。` の行だけで `a` と `b` を作る | 実走 | 実走 | 001 の形で `Ok`（specified、D 6 の RP-70）。落ちた時の扱い（鍵を「同じ」だけに狭める）は D 3.18 の段落が決め、test は a 同士の差と a と b の差を並べて出す。run の順が `a1`、`b1`、`a2`、`b2` でなければ `Err`（run の記録の時刻の順を読む。specified、D 6 の「順に打ち」） | 陽性 | `rp70_translating_without_the_closing_mark_keeps_chrf_and_verify` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- D 6 の RP-70 は、組 `a` と `b` を test-designer の席が git の外に作ると書く。この席は Bash で file を書けず、組の中身は repo に無い。作る手順は、末尾が `。` の行だけを取る `jq`（seed 0 と seed 1 の ja の行）と、その行から `。` を 1 字除く式で、`testdata/baseline/pairs_ending_mark/` と `testdata/baseline/pairs_without_mark/` に置く。この手順を打つ席と、組の行の数を `state.json` に書く席は、設計に名が無い。E2E の席が最初の run の前に打つ。
- chrF の差の幅。H85 のとおり、同じ依頼を 2 回打っても出力の違う行が在る。001 は等号を通すので、偶然の割れで落ちうる。幅を置くかは評議が決める。
- 「止めた行」の数え方：T と E の両方を止めた行を、`translate.line.held` で数えると D は書く。RP-42 の欄が 2 つ在る行がそれに当たると読んだが、`translate.line.held` の欄の綴りは RP-42 の実装が決める。
- 止めた行の `a` の合計と `b` の合計は、`hayamimi-relay eval` の stdout から数える。stdout の置き場は RP-71 と同じ `docs/results/r8w/` の下と読んだが、D に書かれていない。
- seed 0 と seed 1 の組は V の調整に使った組で、held-out ではない（H85）。記録の値（RB-7、RP-13）と比べない。

### 4.69 RP-72（port が移った run を無効にする）

**未実装。** この節の行はどれも、test の file も関数も repo に無い。実装の席が足す。
test：`native/hayamimi-eval/tests/rp72.rs`。受入 test の名前は `rp72_a_run_whose_server_moved_its_port_is_refused`。13 本とも fixture か repo の file を読み、`--ignored` を付けずに通る。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp72`。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。fixture の dir は `native/hayamimi-eval/tests/fixtures/rp72_*` に置く。
設計は D 3.18 の「測り方の直し」の 2 と D 6 の RP-72、H83 である。

**F72**：T-RP-32-030 の dir（3 対、F32 の標本、`pairs.txt` は無い）に、`rs1.events.jsonl`、`rs2.events.jsonl`、`rs3.events.jsonl` を足した物。各 file は 1 行 1 event で、`{"event":{"type":"ready",...},"t_us":N}` の形（`docs/results/rp32/rs1.events.jsonl` と同じ）。`final` を 1 件以上、`fallback` を 0 件持つ。`py<N>.events.jsonl` は置かない。
**in_use の行**：`{"event":{"type":"fallback","stage":"ingest","reason":"in_use","to":"next_port","url":"http://127.0.0.1:8767/ingest"},"t_us":N}`（`native/hayamimi/src/notice.rs` の `ingest_port_in_use` の形）。`stage` を `events` にした物は `port_in_use` の形。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-72-001 | F72 そのまま | 正しい dir。`ports<N>.txt` も無い | `fallback` が 0 件 | `Ok`（specified、D 6 の RP-72 の陽性欄「`fallback` の無い fixture の dir を、port の理由では落とさない」）。`ports<N>.txt` が無くても落ちない（記録で、判定には使わない。D 3.18 の 1） | 陽性 | `a_dir_without_a_port_move_passes` |
| T-RP-72-002 | F72 の `rs2.events.jsonl` に in_use の行（`stage` が `ingest`）を 1 件足す | 1 件で足りる。真ん中の run | 1 件以上 | `Err` で `rs2` を名指し、理由が port の移動（`in_use`）であることを言う。値（`L` の p50 など）は出さない（specified、D 6 の RP-72 の期待値と D 3.18 の 2。今の main では port の理由の `Err` が出ないので、この test は今の main で落ちる） | 陰性 | `a_run_whose_ingest_port_moved_is_refused_by_name` |
| T-RP-72-003 | 002 の in_use の行を `rs1.events.jsonl`（最初）に、次に `rs3.events.jsonl`（最後）に、1 つずつ置く | dir の両端の run | 1 件以上 | どちらも `Err` でその run の名前を挙げる（002 の境界。run の番号の数え方が 2 だけに効いていないことを確かめる） | 境界、陰性 | `the_first_and_the_last_run_are_read_too` |
| T-RP-72-004 | 002 の行の `stage` を `events` にする（`/events` の 8833 が移った形） | 移った port が `/ingest` ではない | 1 件以上 | `Err` で `rs2` を名指す（specified、D 3.18 の 2「`type` が `fallback` で `reason` が `in_use` の event が 1 件でも」。`stage` は条件に入らない） | 陰性 | `a_moved_events_port_is_refused_too` |
| T-RP-72-005 | `rs1.events.jsonl` と `rs3.events.jsonl` の両方に in_use の行を置く | 複数の run | 各 run | `Err` で `rs1` と `rs3` の両方を名指す（specified、D 6 の「その run の名前を挙げて」。1 つ目で止めて 2 つ目を隠さない） | 境界、陰性 | `every_run_that_moved_is_named` |
| T-RP-72-006 | 002 の行の `reason` を `unreadable` にする（`type` は `fallback` のまま） | `type` が合い `reason` が違う | `reason` が `in_use` | `Ok`（specified、D 3.18 の 2 の条件は `type` と `reason` の両方。`REASONS` の別の値） | 境界、陽性 | `a_fallback_for_another_reason_is_not_a_port_move` |
| T-RP-72-007 | 002 の行の `type` を `notice` にする（`reason` は `in_use` のまま） | `reason` が合い `type` が違う | `type` が `fallback` | `Ok`（specified、同上。006 の対） | 境界、陽性 | `an_event_of_another_type_with_in_use_is_not_a_port_move` |
| T-RP-72-008 | F72 の `rs2.events.jsonl` を取り除く | events の file が無い run | file が在る | `Err` で `rs2.events.jsonl` が無いことを名指す。値は出さない（specified、D 6 の RP-72 の期待値「`rs<N>.events.jsonl` の無い dir も、その名前を挙げて `Err`」。確かめられない run を通さない） | 陰性 | `a_run_without_its_events_file_is_refused_by_name` |
| T-RP-72-009 | F72 の `py1.events.jsonl` に in_use の行を置く（`rs<N>` は clean） | Python 側の events | Rust の run だけを読む | `Ok`（specified、D 3.18 の 2「RP-32 の dir の `rs<N>.events.jsonl`」。Python の events は読まない）。`py<N>.events.jsonl` が無くても落ちない（001 と同じ） | 境界、陽性 | `only_the_rust_events_are_read` |
| T-RP-72-010 | `pairs.txt` が `2` の dir（T-RP-32-032 の形）。`rs1.events.jsonl` と `rs2.events.jsonl` は clean、`rs3.events.jsonl` は置かない | 2 対の dir | 在る対の events | `Ok`。対の数が 2 の dir は、`rs3.events.jsonl` を要らない。`rs2.events.jsonl` を取り除くと `Err` で名指す（specified、D 3.16 の 4「`pairs.txt` の数」と 008 の対） | 境界 | `a_two_pair_dir_needs_the_events_of_two_runs_only` |
| T-RP-72-011 | `docs/results/rp32/`（実物の記録）の `rs1.events.jsonl`〜`rs3.events.jsonl` を読む | 実物 | `fallback` が 0 件 | 3 つとも `"type":"fallback"` が 0 件。`"type":"final"` が 141 件（pseudo、`grep -c`。2026-10-20 に `rs1` から `rs3` で `final` 141、141、141、`fallback` と `in_use` は 0 件を数えた）。この dir を `check_dir` に渡した時の `Err` の理由に、port の移動は載らない | 陽性 | `the_recorded_rp32_dir_has_no_port_move` |
| T-RP-72-012 | `docs/results/e2e/README.md` の「Conditions of the run」の節を読む | 手順の文書 | 語が在る | 節の中に `ss -tanH`、`Get-NetTCPConnection`、`8766`、`8833`、`ports<N>.txt` が在る。陽性の対照：同じ file に見出し `Conditions of the run` が 1 件（`rg -c 'Conditions of the run' docs/results/e2e/README.md` が 1）（specified、D 3.18 の「測り方の直し」の 1。今の file には `ss -tanH` が 0 件なので、この test は今の main で落ちる） | 陽性、陰性 | `the_e2e_readme_names_the_port_wait_step` |
| T-RP-72-013 | 受入：002、008、001 の 3 つの dir を順に `check_dir` に渡す | 全体 | 通る | 002 は `Err`（`rs2` の名前）、008 は `Err`（`rs2.events.jsonl` の名前）、001 は `Ok`（specified、D 6 の RP-72） | 陽性、陰性 | `rp72_a_run_whose_server_moved_its_port_is_refused` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 空の `rs<N>.events.jsonl`（0 byte）と、JSON として読めない行を持つ file の扱い。D は「無い run」と「in_use の行が在る run」だけを書く。空の file は run が起きていない形で、読めない行は確かめられない形である。この席は行にしていない。
- `ports<N>.txt` の中身と、待った秒数の上限。D は「記録で、判定には使わない」とだけ書く。001 は、無くても落ちないことだけを固定する。
- 003 の「run の番号の数え方」は `run_names` が 1 から `pairs` まで回すことを指す。実装の関数の綴りを行に写していない。
- H83 のとおり、RP-66 の run の dir（`events.jsonl`）は RP-72 の読み口の外である。
- RP-71 の case 行は書かない（3.18 の段 3 は評議の PAUSE）。

## 5. RP-19 の対応表

Python の `tests/*.py` の 20 本の case と、写した先の Rust の test の名前の対応を、この節の表に置く。
列は、Python の file、case の名前、Rust の test の名前、置き場、の 4 つとする。
RP-19 はこの表の全行を読む。

### 5.1 数えた case の数

数えた日時は 2026-10-13、範囲は `git ls-files 'tests/*.py'` の 20 本である。
数え方は `grep -c "^def test_\|^    def test_"` を file ごとに打ち、足した。

```
$ git ls-files 'tests/*.py' | wc -l
20
$ git ls-files 'tests/*.py' | xargs grep -c '^def test_\|^    def test_' | awk -F: '{s+=$2} END{print "total",s}'
total 386
```

| file | case の数 | file | case の数 |
|---|---|---|---|
| `test_asr_segment.py` | 22 | `test_ja_golden.py` | 4 |
| `test_audio_capture.py` | 19 | `test_ja_pipeline_spec.py` | 4 |
| `test_diar_eval.py` | 20 | `test_lifecycle.py` | 15 |
| `test_diar_osd.py` | 8 | `test_overlay_parity.py` | 1 |
| `test_diar_overlap.py` | 14 | `test_process_safety.py` | 7 |
| `test_diarize.py` | 3 | `test_punct_4class.py` | 14 |
| `test_doc_links.py` | 1 | `test_second_opinion.py` | 8 |
| `test_download_models.py` | 6 | `test_speaker_id.py` | 39 |
| `test_embed_runtime.py` | 67 | `test_units.py` | 99 |
| `test_itn_cjk.py` | 14 | `test_ws_ingest.py` | 21 |

数えた陽性対照：`test_units.py` は `grep -n "^def test_" tests/test_units.py \| wc -l` でも 99 で、`test_embed_runtime.py` は 67 件の名前を `grep -n` で全部読んだ。
`@pytest.mark.parametrize` は `test_audio_capture.py` に 3 本（`input_mode` が 2 値、`shape` が 3 値、`result` が 2 値）、`test_embed_runtime.py` に 2 本（`kwargs` が、無効な値の列と有効な値の列）、`test_itn_cjk.py` に 1 本（`IDIOM_SENTENCES`）ある。
関数の数と pytest が実行する数は一致しない。表は関数の数で数え、parametrize の値の数だけ回す test は Rust の 1 つの test が全部の値を回す形にする。

### 5.2 表の読み方

- Rust の test の名前は、Python の case の名前から先頭の `test_` を除いた物とする。
  `cargo test -- --list` が出す module の path は含めない。名前は表の全行で一意である（`git ls-files 'tests/*.py' \| xargs grep -h '^def test_\|^    def test_' \| sed -E 's/^ *def (test_[A-Za-z0-9_]+).*/\1/' \| sort \| uniq -d` の出力が 0 行。2026-10-13）。
- 置き場は `native/` の下の file で、その file の `#[cfg(test)] mod tests` に test を置く。`tests/` の下の file は、その crate の integration test である。file の名前は設計 3.3 の crate に合わせてこの席が置いた物で、module の割り方は実装の席が変えてよい（変えたら表の置き場の列も同じ手番で直す）。
- Rust の名前が `-` の行は、Rust へ写さない case である。置き場の列に理由が在る。
  - `写さない：<理由>`：3.2 が消すと決めた script の test、設計で無くなる部品（翻訳 worker、翻訳器の pool、文ごとの訳の分割）、外が更新する model の sha256 を gate にする case、Python の実行時の package の import に固有の挙動（Rust には実行時の import が無い）の 4 種。写す先が無い。
- 行の数は 386 で、Rust の名前を持つ行は 334、`写さない` が 52 である。RP-19 の期待値は、名前を持つ 334 行が list に在ることと、`写さない` の 52 行が理由を持つことである。`-` の行は T-RP-19-006 と T-RP-19-007 が理由の有無を確かめる。

### 5.3 対応表

| Python の file | case の名前 | Rust の test の名前 | 置き場（`native/` の下） |
|---|---|---|---|
| `test_asr_segment.py` | `test_join_pieces_concatenates_cjk_without_spaces` | `join_pieces_concatenates_cjk_without_spaces` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_join_pieces_space_joins_word_delimited_languages` | `join_pieces_space_joins_word_delimited_languages` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_join_pieces_drops_empty_and_whitespace_only_pieces` | `join_pieces_drops_empty_and_whitespace_only_pieces` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_fixed_chunks_returns_none_for_short_audio` | `fixed_chunks_returns_none_for_short_audio` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_fixed_chunks_cover_the_whole_buffer_with_overlap` | `fixed_chunks_cover_the_whole_buffer_with_overlap` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_text_density_counts_only_alphanumerics` | `text_density_counts_only_alphanumerics` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_short_buffers_are_never_suspicious` | `short_buffers_are_never_suspicious` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_empty_text_is_not_suspicious` | `empty_text_is_not_suspicious` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_sparse_cjk_output_over_a_long_buffer_is_suspicious` | `sparse_cjk_output_over_a_long_buffer_is_suspicious` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_normal_density_cjk_output_is_not_suspicious` | `normal_density_cjk_output_is_not_suspicious` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_latin_languages_use_their_own_higher_floor` | `latin_languages_use_their_own_higher_floor` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_retry_accepted_when_it_prepends_to_the_surviving_tail` | `retry_accepted_when_it_prepends_to_the_surviving_tail` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_retry_rejected_when_not_longer` | `retry_rejected_when_not_longer` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_retry_rejected_when_it_loses_the_whole_decodes_tail` | `retry_rejected_when_it_loses_the_whole_decodes_tail` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_retry_rejected_when_it_is_still_sparse` | `retry_rejected_when_it_is_still_sparse` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_split_retry_pins_the_recognizer_and_never_reroutes` | `split_retry_pins_the_recognizer_and_never_reroutes` | `hayamimi-asr/src/route.rs` |
| `test_asr_segment.py` | `test_split_retry_keeps_the_whole_text_when_there_is_nothing_to_split` | `split_retry_keeps_the_whole_text_when_there_is_nothing_to_split` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_split_retry_stands_down_when_the_speech_measure_clears_the_buffer_alarm` | `split_retry_stands_down_when_the_speech_measure_clears_the_buffer_alarm` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_a_dropped_multi_utterance_buffer_is_recovered_by_the_retry` | `a_dropped_multi_utterance_buffer_is_recovered_by_the_retry` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_a_non_suspicious_buffer_is_byte_identical_to_the_whole_decode` | `a_non_suspicious_buffer_is_byte_identical_to_the_whole_decode` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_pause_free_buffer_has_nothing_to_split` | `pause_free_buffer_has_nothing_to_split` | `hayamimi-asr/src/decode.rs` |
| `test_asr_segment.py` | `test_live_path_preroll_keeps_utterance_initial_words` | `live_path_preroll_keeps_utterance_initial_words` | `hayamimi/src/stream.rs` |
| `test_audio_capture.py` | `test_cli_rejects_loopback_on_non_windows_before_loading_models` | `cli_rejects_loopback_on_non_windows_before_loading_models` | `hayamimi/src/cli.rs` |
| `test_audio_capture.py` | `test_microphone_device_selection_uses_input_only_case_insensitive_match` | `microphone_device_selection_uses_input_only_case_insensitive_match` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_loopback_rejects_corrupt_channel_or_window_shapes` | `loopback_rejects_corrupt_channel_or_window_shapes` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_windows_com_scope_balances_every_success_result` | `windows_com_scope_balances_every_success_result` | 置き場：`native/hayamimi/src/capture.rs`（COM の初期化と後始末は `hayamimi` が自前の scope で持つ、設計 3.7） |
| `test_audio_capture.py` | `test_windows_com_scope_uses_existing_changed_mode_apartment` | `windows_com_scope_uses_existing_changed_mode_apartment` | 置き場：同上 |
| `test_audio_capture.py` | `test_windows_com_scope_rejects_other_hresult_failures` | `windows_com_scope_rejects_other_hresult_failures` | 置き場：同上 |
| `test_audio_capture.py` | `test_speaker_capture_owns_com_objects_and_downmixes_native_channels` | `speaker_capture_owns_com_objects_and_downmixes_native_channels` | `hayamimi/src/capture.rs`（COM の所有の部分は `wasapi` crate の領分で、downmix の検査だけを書く） |
| `test_audio_capture.py` | `test_speaker_capture_rejects_non_windows_before_import_or_com` | `speaker_capture_rejects_non_windows_before_import_or_com` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_mic_timestamp_is_taken_in_the_capture_callback` | `mic_timestamp_is_taken_in_the_capture_callback` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_speaker_capture_failure_is_reported_with_endpoint_context` | `speaker_capture_failure_is_reported_with_endpoint_context` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_capture_timestamps_keep_audio_spacing_when_backend_returns_a_burst` | `capture_timestamps_keep_audio_spacing_when_backend_returns_a_burst` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_put_latest_keeps_only_newest_bounded_items` | `put_latest_keeps_only_newest_bounded_items` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_aligned_speaker_match_discards_stale_and_retains_future` | `aligned_speaker_match_discards_stale_and_retains_future` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_wait_for_speaker_drains_packet_enqueued_as_done_becomes_true` | `wait_for_speaker_drains_packet_enqueued_as_done_becomes_true` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_mix_stall_drops_both_sides_symmetrically_and_keeps_time_alignment` | `mix_stall_drops_both_sides_symmetrically_and_keeps_time_alignment` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_mix_waits_for_a_closer_packet_instead_of_consuming_older_candidate` | `mix_waits_for_a_closer_packet_instead_of_consuming_older_candidate` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_mix_does_not_retry_failed_soundcard_import_on_capture_thread` | `-` | 写さない：Python の実行時の package の import に固有（`soundcard` の import の再試行）。取り込みの装置が開けない時に止めずに落とす Rust の側の振る舞いは、RP-29 の R5 の行（`capture.fallback.none`）が持つ |
| `test_audio_capture.py` | `test_mix_speaker_failure_falls_back_once_to_mic_only` | `mix_speaker_failure_falls_back_once_to_mic_only` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_mix_microphone_failure_is_fatal_and_stops_speaker` | `mix_microphone_failure_is_fatal_and_stops_speaker` | `hayamimi/src/capture.rs` |
| `test_diar_eval.py` | `test_parse_rttm_basic` | `parse_rttm_basic` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_parse_rttm_ignores_non_speaker_lines_and_blanks` | `parse_rttm_ignores_non_speaker_lines_and_blanks` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_parse_rttm_empty_file` | `parse_rttm_empty_file` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_parse_rttm_rejects_malformed_line` | `parse_rttm_rejects_malformed_line` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_parse_rttm_rejects_negative_duration` | `parse_rttm_rejects_negative_duration` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_parse_rttm_round_trips_zero_duration` | `parse_rttm_round_trips_zero_duration` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_segments_to_der_tuples_coerces_types` | `segments_to_der_tuples_coerces_types` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_segments_to_der_tuples_drops_zero_and_negative_length` | `segments_to_der_tuples_drops_zero_and_negative_length` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_segments_to_der_tuples_empty` | `segments_to_der_tuples_empty` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_der_zero_for_identical_ref_and_hyp` | `der_zero_for_identical_ref_and_hyp` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_der_nonzero_for_swapped_speaker_labels_within_overlap` | `der_nonzero_for_swapped_speaker_labels_within_overlap` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_der_end_to_end_from_parsed_rttm` | `der_end_to_end_from_parsed_rttm` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_der_breakdown_zero_for_identical_ref_and_hyp` | `der_breakdown_zero_for_identical_ref_and_hyp` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_der_breakdown_isolates_confusion_from_miss` | `der_breakdown_isolates_confusion_from_miss` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_group_segments_single_segment` | `group_segments_single_segment` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_group_segments_merges_close_segments` | `group_segments_merges_close_segments` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_group_segments_splits_on_silence_gap` | `group_segments_splits_on_silence_gap` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_group_segments_splits_on_max_length` | `group_segments_splits_on_max_length` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_group_segments_empty_input` | `group_segments_empty_input` | `hayamimi-eval/src/der.rs` |
| `test_diar_eval.py` | `test_production_over_splits_relative_to_eval_replica_on_a_skipped_vad_segment` | `production_over_splits_relative_to_eval_replica_on_a_skipped_vad_segment` | `hayamimi-eval/src/der.rs` |
| `test_diar_osd.py` | `test_pseudo_osd_degenerates_to_plain_argmax` | `-` | 写さない：`eval_diar_osd.py` は 3.2 が消す調査の道具 |
| `test_diar_osd.py` | `test_joint_floor_rejects_lopsided_pair_confidence` | `-` | 写さない：同上 |
| `test_diar_osd.py` | `test_hysteresis_bridges_a_dip_between_thresholds_then_exits` | `-` | 写さない：同上 |
| `test_diar_osd.py` | `test_hysteresis_never_arms_below_hi_thresh` | `-` | 写さない：同上 |
| `test_diar_osd.py` | `test_min_run_frames_rejects_short_accepted_runs` | `-` | 写さない：同上 |
| `test_diar_osd.py` | `test_min_run_frames_keeps_runs_at_least_as_long_as_the_floor` | `-` | 写さない：同上 |
| `test_diar_osd.py` | `test_lo_thresh_above_hi_thresh_is_rejected` | `-` | 写さない：同上 |
| `test_diar_osd.py` | `test_powerset_class_order_still_matches_pyannote` | `-` | 写さない：同上（`eval_diar_overlap.py` の `POWERSET` を読む） |
| `test_diar_overlap.py` | `test_powerset_class_order_matches_pyannote` | `-` | 写さない：`eval_diar_overlap.py` は 3.2 が消す調査の道具 |
| `test_diar_overlap.py` | `test_powerset_decode_singleton_and_pair_and_silence` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_activity_to_segments_bridges_then_drops` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_activity_to_segments_empty` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_strip_overlap_keeps_locally_longer_turn` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_strip_overlap_removes_all_simultaneity` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_overlap_fraction_counts_unavoidable_miss` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_overlap_fraction_zero_without_overlap` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_cluster_respects_cannot_link_within_a_window` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_assign_by_centroid_pulls_unreliable_points_to_nearest_cluster` | `-` | 写さない：同上 |
| `test_diar_overlap.py` | `test_pool_audio_for_group_uses_diarizer_turns_when_present` | `pool_audio_for_group_uses_diarizer_turns_when_present` | `hayamimi-asr/src/recluster.rs` |
| `test_diar_overlap.py` | `test_pool_audio_for_group_falls_back_to_full_buffer_when_declined` | `pool_audio_for_group_falls_back_to_full_buffer_when_declined` | `hayamimi-asr/src/recluster.rs` |
| `test_diar_overlap.py` | `test_pool_audio_for_group_declined_duration_prefers_group_span_over_buffer_length` | `pool_audio_for_group_declined_duration_prefers_group_span_over_buffer_length` | `hayamimi-asr/src/recluster.rs` |
| `test_diar_overlap.py` | `test_pool_audio_for_group_empty_group_yields_empty_audio` | `pool_audio_for_group_empty_group_yields_empty_audio` | `hayamimi-asr/src/recluster.rs` |
| `test_diarize.py` | `test_group_diarizer_missing_model_raises_filenotfound` | `group_diarizer_missing_model_raises_filenotfound` | `hayamimi-asr/src/diarize.rs` |
| `test_diarize.py` | `test_group_diarizer_short_buffer_returns_empty` | `group_diarizer_short_buffer_returns_empty` | `hayamimi-asr/src/diarize.rs` |
| `test_diarize.py` | `test_group_diarizer_finds_multiple_speakers_on_two_speaker_audio` | `group_diarizer_finds_multiple_speakers_on_two_speaker_audio` | `hayamimi-asr/src/diarize.rs` |
| `test_doc_links.py` | `test_no_broken_markdown_links` | `no_broken_markdown_links` | `hayamimi-eval/src/doc_links.rs` |
| `test_download_models.py` | `test_tarball_without_a_wanted_file_raises_and_leaves_no_partial_dir` | `tarball_without_a_wanted_file_raises_and_leaves_no_partial_dir` | `hayamimi/src/models_fetch.rs` |
| `test_download_models.py` | `test_complete_tarball_extracts_only_the_wanted_files` | `complete_tarball_extracts_only_the_wanted_files` | `hayamimi/src/models_fetch.rs` |
| `test_download_models.py` | `test_partial_dir_is_refetched_and_complete_dir_is_skipped` | `partial_dir_is_refetched_and_complete_dir_is_skipped` | `hayamimi/src/models_fetch.rs` |
| `test_download_models.py` | `test_minimal_still_runs_opt_in_downloads` | `minimal_still_runs_opt_in_downloads` | `hayamimi/src/models_fetch.rs` |
| `test_download_models.py` | `test_minimal_with_en_parakeet_v2_downloads_v2` | `minimal_with_en_parakeet_v2_downloads_v2` | `hayamimi/src/models_fetch.rs` |
| `test_download_models.py` | `test_minimal_with_punct_4class_downloads_it` | `minimal_with_punct_4class_downloads_it` | `hayamimi/src/models_fetch.rs` |
| `test_embed_runtime.py` | `test_eventhub_publish_delivers_to_subscriber_in_order` | `eventhub_publish_delivers_to_subscriber_in_order` | `hayamimi/src/events.rs` |
| `test_embed_runtime.py` | `test_eventhub_replays_refine_events_to_new_subscribers` | `eventhub_replays_refine_events_to_new_subscribers` | `hayamimi/src/events.rs` |
| `test_embed_runtime.py` | `test_eventhub_unsubscribe_stops_delivery` | `eventhub_unsubscribe_stops_delivery` | `hayamimi/src/events.rs` |
| `test_embed_runtime.py` | `test_eventhub_add_listener_receives_the_raw_dict_synchronously` | `eventhub_add_listener_receives_the_raw_dict_synchronously` | `hayamimi/src/events.rs` |
| `test_embed_runtime.py` | `test_eventhub_multiple_listeners_all_receive_the_event` | `eventhub_multiple_listeners_all_receive_the_event` | `hayamimi/src/events.rs` |
| `test_embed_runtime.py` | `test_eventhub_listener_exception_is_swallowed_and_reported_once` | `eventhub_listener_exception_is_swallowed_and_reported_once` | `hayamimi/src/events.rs` |
| `test_embed_runtime.py` | `test_eventhub_a_second_broken_listener_gets_its_own_report` | `eventhub_a_second_broken_listener_gets_its_own_report` | `hayamimi/src/events.rs` |
| `test_embed_runtime.py` | `test_final_event_carries_audio_s_lid_ms_decode_ms` | `final_event_carries_audio_s_lid_ms_decode_ms` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_final_event_switched_is_false_for_the_first_final` | `final_event_switched_is_false_for_the_first_final` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_final_event_switched_true_when_language_changes` | `final_event_switched_true_when_language_changes` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_final_event_switched_false_when_language_repeats` | `final_event_switched_false_when_language_repeats` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_session_stats_reset_clears_last_final_lang` | `session_stats_reset_clears_last_final_lang` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_translation_worker_publishes_on_hub_with_no_http_server` | `-` | 写さない：Python の in-process の翻訳 worker はこの設計で無くなり、訳は `hayamimi-relay` が出す（設計 3.6、RP-19）。relay の側へは写さない。中継が訳を自分の `/events` に出すことは会話字幕の CS-21 と CS-22 が持つ |
| `test_embed_runtime.py` | `test_translation_worker_is_a_silent_no_op_with_an_empty_pool` | `-` | 写さない：同上 |
| `test_embed_runtime.py` | `test_set_forced_lang_none_reverts_to_auto` | `set_forced_lang_none_reverts_to_auto` | `hayamimi-asr/src/route.rs` |
| `test_embed_runtime.py` | `test_set_forced_lang_accepts_a_routable_code` | `set_forced_lang_accepts_a_routable_code` | `hayamimi-asr/src/route.rs` |
| `test_embed_runtime.py` | `test_set_forced_lang_rejects_an_unroutable_code` | `set_forced_lang_rejects_an_unroutable_code` | `hayamimi-asr/src/route.rs` |
| `test_embed_runtime.py` | `test_set_forced_lang_resets_sticky_and_pending_lid_state` | `set_forced_lang_resets_sticky_and_pending_lid_state` | `hayamimi-asr/src/route.rs` |
| `test_embed_runtime.py` | `test_set_dual_confirm_updates_the_flag` | `set_dual_confirm_updates_the_flag` | `hayamimi-asr/src/route.rs` |
| `test_embed_runtime.py` | `test_set_lid_switch_confirm_accepts_valid_and_rejects_below_one` | `set_lid_switch_confirm_accepts_valid_and_rejects_below_one` | `hayamimi-asr/src/route.rs` |
| `test_embed_runtime.py` | `test_set_min_switch_s_accepts_valid_and_rejects_negative` | `set_min_switch_s_accepts_valid_and_rejects_negative` | `hayamimi-asr/src/route.rs` |
| `test_embed_runtime.py` | `test_set_punctuate_off_then_on_is_a_noop_when_already_loaded` | `set_punctuate_off_then_on_is_a_noop_when_already_loaded` | `hayamimi-asr/src/punct.rs` |
| `test_embed_runtime.py` | `test_set_punctuate_reenable_after_degrade_clears_punct_for_retry` | `set_punctuate_reenable_after_degrade_clears_punct_for_retry` | `hayamimi-asr/src/punct.rs` |
| `test_embed_runtime.py` | `test_translator_pool_add_target_builds_once_and_emits_model_load` | `-` | 写さない：翻訳器の pool はこの設計で無くなり、`--translate` の言語は `hayamimi-relay` が llama-server で訳す（設計 3.6、R8f、RP-13、RP-19） |
| `test_embed_runtime.py` | `test_translator_pool_add_target_is_idempotent` | `-` | 写さない：同上 |
| `test_embed_runtime.py` | `test_translator_pool_remove_target_drops_it` | `-` | 写さない：同上 |
| `test_embed_runtime.py` | `test_translator_pool_remove_target_missing_is_a_noop` | `-` | 写さない：同上 |
| `test_embed_runtime.py` | `test_translator_pool_add_target_unsupported_raises_value_error` | `-` | 写さない：同上 |
| `test_embed_runtime.py` | `test_build_translators_degrades_unsupported_target_to_stderr` | `-` | 写さない：同上 |
| `test_embed_runtime.py` | `test_translator_pool_items_and_targets_survive_concurrent_add_remove` | `-` | 写さない：同上 |
| `test_embed_runtime.py` | `test_live_vad_applies_pending_change_at_a_safe_boundary` | `live_vad_applies_pending_change_at_a_safe_boundary` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_defers_rebuild_while_speech_is_active` | `live_vad_defers_rebuild_while_speech_is_active` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_set_sensitivity_partial_args_keep_other_values` | `live_vad_set_sensitivity_partial_args_keep_other_values` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_composes_pending_updates_and_keeps_last_explicit_value` | `live_vad_composes_pending_updates_and_keeps_last_explicit_value` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_empty_update_does_not_rebuild_detector` | `live_vad_empty_update_does_not_rebuild_detector` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_waits_for_completed_segment_to_be_drained` | `live_vad_waits_for_completed_segment_to_be_drained` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_rebuild_preserves_absolute_segment_positions` | `live_vad_rebuild_preserves_absolute_segment_positions` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_failed_rebuild_preserves_active_and_pending_state` | `live_vad_failed_rebuild_preserves_active_and_pending_state` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_rebuild_keeps_stream_preroll_and_refiner_coordinates` | `live_vad_rebuild_keeps_stream_preroll_and_refiner_coordinates` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_real_speech_matches_fresh_detector_after_rebuild` | `live_vad_real_speech_matches_fresh_detector_after_rebuild` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_set_sensitivity_rejects_invalid_values` | `live_vad_set_sensitivity_rejects_invalid_values` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_set_sensitivity_accepts_valid_values` | `live_vad_set_sensitivity_accepts_valid_values` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_live_vad_set_sensitivity_none_is_always_a_noop_no_change` | `live_vad_set_sensitivity_none_is_always_a_noop_no_change` | `hayamimi-asr/src/vad.rs` |
| `test_embed_runtime.py` | `test_post_config_invalid_vad_value_returns_400_not_a_crash` | `post_config_invalid_vad_value_returns_400_not_a_crash` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_speaker_labeler_reset_clears_everything` | `speaker_labeler_reset_clears_everything` | `hayamimi-asr/src/speaker.rs` |
| `test_embed_runtime.py` | `test_audio_history_clear_empties_buffer_and_offsets` | `audio_history_clear_empties_buffer_and_offsets` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_refiner_reset_drops_spans_and_clears_history` | `refiner_reset_drops_spans_and_clears_history` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_refiner_reset_writes_a_separator_into_an_open_transcript` | `refiner_reset_writes_a_separator_into_an_open_transcript` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_reset_live_session_emits_summary_then_reset_in_order` | `reset_live_session_emits_summary_then_reset_in_order` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_reset_live_session_tolerates_every_optional_piece_being_none` | `reset_live_session_tolerates_every_optional_piece_being_none` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_reset_live_session_flushes_pending_refine_before_summarizing` | `reset_live_session_flushes_pending_refine_before_summarizing` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_control_queue_submit_does_not_run_synchronously` | `control_queue_submit_does_not_run_synchronously` | `hayamimi/src/control.rs` |
| `test_embed_runtime.py` | `test_control_queue_poll_runs_queued_callables_in_fifo_order` | `control_queue_poll_runs_queued_callables_in_fifo_order` | `hayamimi/src/control.rs` |
| `test_embed_runtime.py` | `test_control_queue_poll_with_nothing_queued_is_a_noop` | `control_queue_poll_with_nothing_queued_is_a_noop` | `hayamimi/src/control.rs` |
| `test_embed_runtime.py` | `test_control_queue_poll_sets_the_event_after_running` | `control_queue_poll_sets_the_event_after_running` | `hayamimi/src/control.rs` |
| `test_embed_runtime.py` | `test_control_queue_poll_sets_event_and_continues_past_a_raising_callable` | `control_queue_poll_sets_event_and_continues_past_a_raising_callable` | `hayamimi/src/control.rs` |
| `test_embed_runtime.py` | `test_control_queue_task_runs_on_the_thread_that_calls_poll_not_the_submitter` | `control_queue_task_runs_on_the_thread_that_calls_poll_not_the_submitter` | `hayamimi/src/control.rs` |
| `test_embed_runtime.py` | `test_run_stream_polls_control_once_per_chunk` | `run_stream_polls_control_once_per_chunk` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_run_stream_without_control_is_unaffected` | `run_stream_without_control_is_unaffected` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_run_stream_applies_a_submitted_task_on_the_decode_thread_not_the_submitter` | `run_stream_applies_a_submitted_task_on_the_decode_thread_not_the_submitter` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_config_and_reset_404_without_controls_attached` | `config_and_reset_404_without_controls_attached` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_get_config_reports_current_values` | `get_config_reports_current_values` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_config_applies_a_subset_of_keys` | `post_config_applies_a_subset_of_keys` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_config_invalid_value_returns_400_with_message` | `post_config_invalid_value_returns_400_with_message` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_reset_calls_the_reset_function` | `post_reset_calls_the_reset_function` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_reset_pending_returns_202` | `post_reset_pending_returns_202` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_reset_without_reset_fn_returns_400` | `post_reset_without_reset_fn_returns_400` | `hayamimi/src/server.rs` |
| `test_itn_cjk.py` | `test_only_applies_to_ja_zh_yue` | `only_applies_to_ja_zh_yue` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_empty_text_is_a_noop` | `empty_text_is_a_noop` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_magnitude_form_full_arithmetic_without_big_units` | `magnitude_form_full_arithmetic_without_big_units` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_magnitude_form_keeps_man_and_oku_as_literal_units` | `magnitude_form_keeps_man_and_oku_as_literal_units` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_traditional_wan_and_liang_forms` | `traditional_wan_and_liang_forms` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_decimal_digit_by_digit_reading` | `decimal_digit_by_digit_reading` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_decimal_with_magnitude_integer_part` | `decimal_with_magnitude_integer_part` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_bare_digit_run_reads_digit_by_digit` | `bare_digit_run_reads_digit_by_digit` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_single_bare_numeral_untouched` | `single_bare_numeral_untouched` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_idioms_and_proper_nouns_stay_untouched` | `idioms_and_proper_nouns_stay_untouched` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_default_exclusion_list_blocks_a_longer_run_directly` | `default_exclusion_list_blocks_a_longer_run_directly` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_user_exclude_adds_to_builtin_list` | `user_exclude_adds_to_builtin_list` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_user_force_wins_over_rule_output` | `user_force_wins_over_rule_output` | `hayamimi-asr/src/itn.rs` |
| `test_itn_cjk.py` | `test_user_force_can_apply_to_text_the_rules_would_leave_alone` | `user_force_can_apply_to_text_the_rules_would_leave_alone` | `hayamimi-asr/src/itn.rs` |
| `test_ja_golden.py` | `test_golden_file_is_self_consistent` | `golden_file_is_self_consistent` | `hayamimi-asr/tests/ja_golden.rs` |
| `test_ja_golden.py` | `test_golden_wavs_are_the_ones_that_were_recorded` | `golden_wavs_are_the_ones_that_were_recorded` | `hayamimi-asr/tests/ja_golden.rs` |
| `test_ja_golden.py` | `test_references_still_match_the_fleurs_manifest` | `references_still_match_the_fleurs_manifest` | `hayamimi-asr/tests/ja_golden.rs` |
| `test_ja_golden.py` | `test_pipeline_still_produces_the_golden_text` | `pipeline_still_produces_the_golden_text` | `hayamimi-asr/tests/ja_golden.rs` |
| `test_ja_pipeline_spec.py` | `test_spec_file_exists_and_declares_its_generator` | `spec_file_exists_and_declares_its_generator` | `hayamimi/tests/ja_pipeline_spec.rs` |
| `test_ja_pipeline_spec.py` | `test_config_matches_the_live_dump` | `config_matches_the_live_dump` | `hayamimi/tests/ja_pipeline_spec.rs` |
| `test_ja_pipeline_spec.py` | `test_dump_needs_no_models_directory` | `dump_needs_no_models_directory` | `hayamimi/tests/ja_pipeline_spec.rs` |
| `test_ja_pipeline_spec.py` | `test_model_hashes_match_when_models_are_present` | `-` | 写さない：外が更新する model の sha256 を動作の gate にしない。sha256 は `models/manifest.json` の記録に留め、在ることだけを RP-21 が見る（設計 3.10、RP-19） |
| `test_lifecycle.py` | `test_refiner_close_ends_the_worker_thread` | `refiner_close_ends_the_worker_thread` | `hayamimi/src/stream.rs` |
| `test_lifecycle.py` | `test_refiner_close_is_idempotent` | `refiner_close_is_idempotent` | `hayamimi/src/stream.rs` |
| `test_lifecycle.py` | `test_refiner_close_runs_the_queued_backlog_first` | `refiner_close_runs_the_queued_backlog_first` | `hayamimi/src/stream.rs` |
| `test_lifecycle.py` | `test_refiner_refuses_work_after_close` | `refiner_refuses_work_after_close` | `hayamimi/src/stream.rs` |
| `test_lifecycle.py` | `test_closing_the_refiner_releases_the_engine` | `closing_the_refiner_releases_the_engine` | `hayamimi/src/stream.rs` |
| `test_lifecycle.py` | `test_refiner_close_closes_the_transcript_file` | `refiner_close_closes_the_transcript_file` | `hayamimi/src/stream.rs` |
| `test_lifecycle.py` | `test_translation_worker_close_ends_its_thread` | `-` | 写さない：翻訳 worker はこの設計で無くなる（`test_embed_runtime.py` の翻訳 worker の行と同じ） |
| `test_lifecycle.py` | `test_translation_worker_submit_after_close_is_a_no_op` | `-` | 写さない：同上 |
| `test_lifecycle.py` | `test_engine_close_joins_the_preload_thread` | `engine_close_joins_the_preload_thread` | `hayamimi-asr/src/route.rs` |
| `test_lifecycle.py` | `test_engine_close_drops_every_model_reference` | `engine_close_drops_every_model_reference` | `hayamimi-asr/src/route.rs` |
| `test_lifecycle.py` | `test_engine_close_is_idempotent` | `engine_close_is_idempotent` | `hayamimi-asr/src/route.rs` |
| `test_lifecycle.py` | `test_engine_refuses_work_after_close` | `engine_refuses_work_after_close` | `hayamimi-asr/src/route.rs` |
| `test_lifecycle.py` | `test_closed_engine_starts_no_new_background_work` | `closed_engine_starts_no_new_background_work` | `hayamimi-asr/src/route.rs` |
| `test_lifecycle.py` | `test_closed_engine_is_collectable` | `closed_engine_is_collectable` | `hayamimi-asr/src/route.rs` |
| `test_lifecycle.py` | `test_run_clip_leaves_no_engine_behind` | `run_clip_leaves_no_engine_behind` | `hayamimi/src/stream.rs` |
| `test_overlay_parity.py` | `test_overlay_html_matches_mobile_port` | `overlay_html_matches_mobile_port` | `hayamimi-ui/tests/overlay_parity.rs` |
| `test_process_safety.py` | `test_routed_asr_init_raises_not_exits_when_lid_missing` | `routed_asr_init_raises_not_exits_when_lid_missing` | `hayamimi-asr/src/route.rs` |
| `test_process_safety.py` | `test_build_lid_guarded_never_reaches_sherpa_onnx_when_missing` | `build_lid_guarded_never_reaches_sherpa_onnx_when_missing` | `hayamimi-asr/src/lid.rs` |
| `test_process_safety.py` | `test_build_lid_guarded_builds_normally_when_present` | `build_lid_guarded_builds_normally_when_present` | `hayamimi-asr/src/lid.rs` |
| `test_process_safety.py` | `test_build_vad_raises_not_exits_when_model_missing` | `build_vad_raises_not_exits_when_model_missing` | `hayamimi-asr/src/vad.rs` |
| `test_process_safety.py` | `test_run_stream_stops_mid_wav_when_stop_event_is_set` | `run_stream_stops_mid_wav_when_stop_event_is_set` | `hayamimi/src/stream.rs` |
| `test_process_safety.py` | `test_run_stream_without_stop_event_runs_to_completion` | `run_stream_without_stop_event_runs_to_completion` | `hayamimi/src/stream.rs` |
| `test_process_safety.py` | `test_mic_chunks_notices_stop_event_without_audio` | `mic_chunks_notices_stop_event_without_audio` | `hayamimi/src/capture.rs` |
| `test_punct_4class.py` | `test_reconstruct_inserts_period_and_comma_after_token_end` | `reconstruct_inserts_period_and_comma_after_token_end` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_reconstruct_inserts_question_mark` | `reconstruct_inserts_question_mark` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_reconstruct_inserts_exclamation_mark` | `reconstruct_inserts_exclamation_mark` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_reconstruct_no_mark_is_noop` | `reconstruct_no_mark_is_noop` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_reconstruct_ignores_special_token_offsets` | `reconstruct_ignores_special_token_offsets` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_reconstruct_only_first_mark_per_position_wins` | `reconstruct_only_first_mark_per_position_wins` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_reconstruct_multi_sentence_marks_mid_and_end` | `reconstruct_multi_sentence_marks_mid_and_end` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_punct4_labels_order` | `punct4_labels_order` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_restore_smoke_question` | `restore_smoke_question` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_restore_smoke_exclamation` | `restore_smoke_exclamation` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_restore_smoke_only_inserts_marks` | `restore_smoke_only_inserts_marks` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_restore_smoke_empty_string` | `restore_smoke_empty_string` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_missing_model_dir_raises_file_not_found` | `missing_model_dir_raises_file_not_found` | `hayamimi-asr/src/punct.rs` |
| `test_punct_4class.py` | `test_decode_gates_remove_weak_marks_before_argmax` | `decode_gates_remove_weak_marks_before_argmax` | `hayamimi-asr/src/punct.rs` |
| `test_second_opinion.py` | `test_agreement_cer_ignores_spaces_and_punct` | `agreement_cer_ignores_spaces_and_punct` | `hayamimi-asr/src/route.rs` |
| `test_second_opinion.py` | `test_agreement_cer_scales_with_divergence` | `agreement_cer_scales_with_divergence` | `hayamimi-asr/src/route.rs` |
| `test_second_opinion.py` | `test_choose_adopts_second_when_close` | `choose_adopts_second_when_close` | `hayamimi-asr/src/route.rs` |
| `test_second_opinion.py` | `test_choose_keeps_primary_when_divergent` | `choose_keeps_primary_when_divergent` | `hayamimi-asr/src/route.rs` |
| `test_second_opinion.py` | `test_choose_keeps_primary_on_empty_second` | `choose_keeps_primary_on_empty_second` | `hayamimi-asr/src/route.rs` |
| `test_second_opinion.py` | `test_missing_model_warns_once_and_disables` | `missing_model_warns_once_and_disables` | `hayamimi-asr/src/route.rs` |
| `test_second_opinion.py` | `test_second_decode_failure_keeps_primary` | `second_decode_failure_keeps_primary` | `hayamimi-asr/src/route.rs` |
| `test_second_opinion.py` | `test_agreeing_second_is_adopted` | `agreeing_second_is_adopted` | `hayamimi-asr/src/route.rs` |
| `test_speaker_id.py` | `test_match_embedding_opens_first_speaker` | `match_embedding_opens_first_speaker` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_match_embedding_joins_existing_within_threshold` | `match_embedding_joins_existing_within_threshold` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_match_embedding_opens_new_speaker_below_threshold` | `match_embedding_opens_new_speaker_below_threshold` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_match_embedding_update_false_does_not_mutate_on_hit` | `match_embedding_update_false_does_not_mutate_on_hit` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_match_embedding_update_false_returns_empty_on_miss` | `match_embedding_update_false_returns_empty_on_miss` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_match_embedding_picks_nearest_of_several_centroids` | `match_embedding_picks_nearest_of_several_centroids` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_exclude_provisional_skips_a_one_off_centroid_as_a_match_target` | `exclude_provisional_skips_a_one_off_centroid_as_a_match_target` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_exclude_provisional_still_matches_a_confirmed_centroid` | `exclude_provisional_still_matches_a_confirmed_centroid` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_exclude_provisional_false_is_a_no_op_default` | `exclude_provisional_false_is_a_no_op_default` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_joint_remap_single_cluster_matches_independent_match_embedding` | `joint_remap_single_cluster_matches_independent_match_embedding` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_joint_remap_no_live_centroids_opens_distinct_new_speakers` | `joint_remap_no_live_centroids_opens_distinct_new_speakers` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_joint_remap_keeps_two_clusters_on_distinct_speakers_when_both_eligible` | `joint_remap_keeps_two_clusters_on_distinct_speakers_when_both_eligible` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_joint_remap_falls_back_to_independent_match_when_below_threshold` | `joint_remap_falls_back_to_independent_match_when_below_threshold` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_joint_remap_update_false_does_not_mutate_centroids` | `joint_remap_update_false_does_not_mutate_centroids` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_joint_remap_empty_input_returns_empty_list` | `joint_remap_empty_input_returns_empty_list` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_match_embedding_default_source_is_empty_and_only_logged_on_open` | `match_embedding_default_source_is_empty_and_only_logged_on_open` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_centroid_open_counts_tags_fast_and_remap_sources_separately` | `centroid_open_counts_tags_fast_and_remap_sources_separately` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_label_forwards_source_to_match_embedding` | `label_forwards_source_to_match_embedding` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_centroid_summary_flags_one_off_opens_with_a_final_match_count_of_one` | `centroid_summary_flags_one_off_opens_with_a_final_match_count_of_one` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_new_centroid_is_provisional_until_second_hit` | `new_centroid_is_provisional_until_second_hit` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_confirmed_centroid_stays_confirmed_after_more_hits` | `confirmed_centroid_stays_confirmed_after_more_hits` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_display_label_does_not_affect_which_centroid_is_assigned` | `display_label_does_not_affect_which_centroid_is_assigned` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_display_label_empty_string_passthrough` | `display_label_empty_string_passthrough` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_display_label_out_of_range_label_passthrough` | `display_label_out_of_range_label_passthrough` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_provisional_confirm_hits_constant_is_two` | `provisional_confirm_hits_constant_is_two` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_provisional_label_count_counts_only_still_provisional_centroids` | `provisional_label_count_counts_only_still_provisional_centroids` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_provisional_label_count_skips_merged_away_centroids` | `provisional_label_count_skips_merged_away_centroids` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_hysteresis_first_speaker_confirmed_immediately` | `hysteresis_first_speaker_confirmed_immediately` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_hysteresis_provisional_speaker_falls_back_to_nearest_confirmed` | `hysteresis_provisional_speaker_falls_back_to_nearest_confirmed` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_hysteresis_confirms_after_min_hits_and_gets_own_label` | `hysteresis_confirms_after_min_hits_and_gets_own_label` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_hysteresis_disabled_opens_new_speaker_immediately` | `hysteresis_disabled_opens_new_speaker_immediately` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_merge_centroids_folds_similar_speakers` | `merge_centroids_folds_similar_speakers` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_merge_centroids_below_threshold_does_nothing` | `merge_centroids_below_threshold_does_nothing` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_maybe_merge_centroids_is_noop_when_disabled` | `maybe_merge_centroids_is_noop_when_disabled` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_merged_centroid_absorbs_future_matches` | `merged_centroid_absorbs_future_matches` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_merge_history_flattens_across_calls` | `merge_history_flattens_across_calls` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_fast_path_default_finds_both_real_speakers` | `fast_path_default_finds_both_real_speakers` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_provisional_display_does_not_reproduce_section_9s_swallowing_bug` | `provisional_display_does_not_reproduce_section_9s_swallowing_bug` | `hayamimi-asr/src/speaker.rs` |
| `test_speaker_id.py` | `test_hysteresis_can_swallow_a_rare_real_speaker` | `hysteresis_can_swallow_a_rare_real_speaker` | `hayamimi-asr/src/speaker.rs` |
| `test_units.py` | `test_digits_pass_when_source_has_no_ascii_digits` | `digits_pass_when_source_has_no_ascii_digits` | 置き場：`native/hayamimi-relay` の照合 V の test（会話字幕の CS-4a）。数字の守りは V が持つ |
| `test_units.py` | `test_digits_pass_on_exact_and_superset` | `digits_pass_on_exact_and_superset` | 置き場：同上 |
| `test_units.py` | `test_digits_fail_on_mangled_number` | `digits_fail_on_mangled_number` | 置き場：同上 |
| `test_units.py` | `test_translate_by_sentence_splits_on_terminators` | `-` | 写さない：文ごとの訳の分割はこの設計で無くなる（訳は relay が 1 行ずつ頼む、設計 3.6、RP-19） |
| `test_units.py` | `test_translate_by_sentence_drops_untranslated` | `-` | 写さない：同上 |
| `test_units.py` | `test_translate_by_sentence_number_guard_falls_back` | `-` | 写さない：同上 |
| `test_units.py` | `test_preroll_prepends_context` | `preroll_prepends_context` | `hayamimi/src/stream.rs` |
| `test_units.py` | `test_preroll_never_bleeds_into_previous_segment` | `preroll_never_bleeds_into_previous_segment` | `hayamimi/src/stream.rs` |
| `test_units.py` | `test_preroll_respects_rolling_buffer_offset` | `preroll_respects_rolling_buffer_offset` | `hayamimi/src/stream.rs` |
| `test_units.py` | `test_load_replacements_formats` | `load_replacements_formats` | `hayamimi-asr/src/punct.rs` |
| `test_units.py` | `test_load_replacements_empty_path` | `load_replacements_empty_path` | `hayamimi-asr/src/punct.rs` |
| `test_units.py` | `test_cjkchar_units_splits_cjk_chars_and_keeps_ascii_words` | `cjkchar_units_splits_cjk_chars_and_keeps_ascii_words` | `hayamimi-asr/src/decode.rs` |
| `test_units.py` | `test_check_hotwords_encodable_all_missing` | `check_hotwords_encodable_all_missing` | `hayamimi-asr/src/decode.rs` |
| `test_units.py` | `test_check_hotwords_encodable_some_found` | `check_hotwords_encodable_some_found` | `hayamimi-asr/src/decode.rs` |
| `test_units.py` | `test_check_hotwords_encodable_empty_path` | `check_hotwords_encodable_empty_path` | `hayamimi-asr/src/decode.rs` |
| `test_units.py` | `test_is_supported_target_accepts_known_m2m100_codes` | `-` | 写さない：`translate_m2m.py` は 3.2 が消す（訳は GPU の言語モデル） |
| `test_units.py` | `test_is_supported_target_rejects_unknown_code` | `-` | 写さない：同上 |
| `test_units.py` | `test_is_supported_target_missing_model_dir_returns_false` | `-` | 写さない：同上 |
| `test_units.py` | `test_validated_targets_are_a_subset_of_supported` | `-` | 写さない：同上 |
| `test_units.py` | `test_validated_targets_tier_matches_measured_set` | `-` | 写さない：同上 |
| `test_units.py` | `test_translator_rejects_unsupported_target_before_loading_model` | `-` | 写さない：同上 |
| `test_units.py` | `test_default_beam_size_used_for_unvalidated_targets` | `-` | 写さない：同上 |
| `test_units.py` | `test_make_translator_rejects_unknown_backend` | `-` | 写さない：`translate_candidates.py` は 3.2 が消す調査の道具 |
| `test_units.py` | `test_make_translator_opus_mt_rejects_target_with_no_upstream_model` | `-` | 写さない：同上 |
| `test_units.py` | `test_make_translator_fugumt_rejects_non_en_target` | `-` | 写さない：同上 |
| `test_units.py` | `test_opus_mt_dir_by_target_only_has_en_and_es` | `-` | 写さない：同上 |
| `test_units.py` | `test_capped_max_length_is_bounded_both_directions` | `-` | 写さない：同上 |
| `test_units.py` | `test_routing_sets_are_disjoint` | `routing_sets_are_disjoint` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_expected_language_homes` | `expected_language_homes` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_resolve_en_tier_accepts_known_values` | `resolve_en_tier_accepts_known_values` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_resolve_en_tier_rejects_unknown_value` | `resolve_en_tier_rejects_unknown_value` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_route_en_stays_on_v3_by_default` | `route_en_stays_on_v3_by_default` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_route_en_switches_to_v2_when_opted_in` | `route_en_switches_to_v2_when_opted_in` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_route_other_v3_langs_unaffected_by_en_tier` | `route_other_v3_langs_unaffected_by_en_tier` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_route_en_v2_missing_degrades_to_v3_not_rz` | `route_en_v2_missing_degrades_to_v3_not_rz` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_route_en_v2_present_uses_v2` | `route_en_v2_present_uses_v2` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_get_with_fallback_generic_chain_unchanged` | `get_with_fallback_generic_chain_unchanged` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_resolve_punct_model_accepts_known_values` | `resolve_punct_model_accepts_known_values` | `hayamimi-asr/src/punct.rs` |
| `test_units.py` | `test_resolve_punct_model_rejects_unknown_value` | `resolve_punct_model_rejects_unknown_value` | `hayamimi-asr/src/punct.rs` |
| `test_units.py` | `test_punct_property_defaults_to_bert` | `punct_property_defaults_to_bert` | `hayamimi-asr/src/punct.rs` |
| `test_units.py` | `test_punct_property_builds_4class_when_selected` | `punct_property_builds_4class_when_selected` | `hayamimi-asr/src/punct.rs` |
| `test_units.py` | `test_punct_property_4class_missing_falls_back_to_bert_with_warning` | `punct_property_4class_missing_falls_back_to_bert_with_warning` | `hayamimi-asr/src/punct.rs` |
| `test_units.py` | `test_punct_property_disables_when_both_models_unavailable` | `punct_property_disables_when_both_models_unavailable` | `hayamimi-asr/src/punct.rs` |
| `test_units.py` | `test_script_correction_matrix` | `script_correction_matrix` | `hayamimi-asr/src/lid.rs` |
| `test_units.py` | `test_has_kana` | `has_kana` | `hayamimi-asr/src/lid.rs` |
| `test_units.py` | `test_sticky_first_utterance_has_no_last_lang_yet` | `sticky_first_utterance_has_no_last_lang_yet` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_bootstrap_prefers_probe_language_over_unarbitrable_guess` | `sticky_bootstrap_prefers_probe_language_over_unarbitrable_guess` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_bootstrap_prefers_probe_language_for_long_unarbitrable_guess` | `sticky_bootstrap_prefers_probe_language_for_long_unarbitrable_guess` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_bootstrap_non_sv_lang_confirms_after_repeats` | `sticky_bootstrap_non_sv_lang_confirms_after_repeats` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_same_lang_resets_pending` | `sticky_same_lang_resets_pending` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_single_misfire_is_held_not_switched` | `sticky_single_misfire_is_held_not_switched` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_short_misfire_suppresses_fallback` | `sticky_short_misfire_suppresses_fallback` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_confirmed_switch_after_two_consecutive_detections` | `sticky_confirmed_switch_after_two_consecutive_detections` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_alternating_misfires_never_accumulate` | `sticky_alternating_misfires_never_accumulate` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_partial_forced_lang_routes_directly_without_lid_or_sv_probe` | `partial_forced_lang_routes_directly_without_lid_or_sv_probe` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_reset_session_clears_sticky_state` | `reset_session_clears_sticky_state` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_switch_confirm_one_disables_hysteresis` | `sticky_switch_confirm_one_disables_hysteresis` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_switch_confirm_one_and_guard_zero_fully_disable_the_lock` | `switch_confirm_one_and_guard_zero_fully_disable_the_lock` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_short_detection_never_advances_pending_count` | `sticky_short_detection_never_advances_pending_count` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_short_detections_never_confirm_a_switch` | `sticky_short_detections_never_confirm_a_switch` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_short_detection_does_not_reset_a_real_candidate` | `sticky_short_detection_does_not_reset_a_real_candidate` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sv_lid_tag_normalizes_bracketed_tag` | `sv_lid_tag_normalizes_bracketed_tag` | `hayamimi-asr/src/lid.rs` |
| `test_units.py` | `test_dual_confirm_same_lang_is_a_noop` | `dual_confirm_same_lang_is_a_noop` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_dual_confirm_bootstrap_trusts_probe_over_whisper_misfire` | `dual_confirm_bootstrap_trusts_probe_over_whisper_misfire` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_dual_confirm_bootstrap_agreement_marks_switched` | `dual_confirm_bootstrap_agreement_marks_switched` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_dual_confirm_holds_current_lang_on_disagreement` | `dual_confirm_holds_current_lang_on_disagreement` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_dual_confirm_switches_immediately_on_agreement` | `dual_confirm_switches_immediately_on_agreement` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_dual_confirm_ignores_sub_probe_length_even_on_agreement` | `dual_confirm_ignores_sub_probe_length_even_on_agreement` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_dual_confirm_zh_whisper_yue_sensevoice_switches_to_yue` | `dual_confirm_zh_whisper_yue_sensevoice_switches_to_yue` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_transcribe_bootstrap_too_short_decodes_but_does_not_seed_last_lang` | `transcribe_bootstrap_too_short_decodes_but_does_not_seed_last_lang` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_transcribe_bootstrap_zh_yue_reuses_single_sv_probe_decode` | `transcribe_bootstrap_zh_yue_reuses_single_sv_probe_decode` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_dual_confirm_session_switches_from_each_stuck_lang_to_yue` | `dual_confirm_session_switches_from_each_stuck_lang_to_yue` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_dual_confirm_mismatch_at_bootstrap_falls_back_to_whisper_guess` | `dual_confirm_mismatch_at_bootstrap_falls_back_to_whisper_guess` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_sticky_long_detections_still_confirm_a_switch_under_large_guard` | `sticky_long_detections_still_confirm_a_switch_under_large_guard` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_refine_short_solo_group_never_reconsiders_language` | `refine_short_solo_group_never_reconsiders_language` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_refine_disagreement_without_sv_confirmation_keeps_current_lang` | `refine_disagreement_without_sv_confirmation_keeps_current_lang` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_refine_agreement_above_length_gate_overrides` | `refine_agreement_above_length_gate_overrides` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_refine_same_lang_is_a_noop` | `refine_same_lang_is_a_noop` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_refine_agreement_below_length_gate_still_does_not_override` | `refine_agreement_below_length_gate_still_does_not_override` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_refine_reuses_sv_probe_text_for_ko_yue_instead_of_redecoding` | `refine_reuses_sv_probe_text_for_ko_yue_instead_of_redecoding` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_refiner_add_span_keeps_same_language_in_one_group` | `refiner_add_span_keeps_same_language_in_one_group` | `hayamimi/src/stream.rs` |
| `test_units.py` | `test_refiner_add_span_splits_group_at_language_boundary` | `refiner_add_span_splits_group_at_language_boundary` | `hayamimi/src/stream.rs` |
| `test_units.py` | `test_refiner_add_span_splits_again_on_return_to_original_language` | `refiner_add_span_splits_again_on_return_to_original_language` | `hayamimi/src/stream.rs` |
| `test_units.py` | `test_refiner_worker_prints_groups_in_enqueue_order_despite_slower_first_decode` | `refiner_worker_prints_groups_in_enqueue_order_despite_slower_first_decode` | `hayamimi/src/stream.rs` |
| `test_units.py` | `test_refiner_shutdown_must_drain_full_backlog_not_just_last_task` | `refiner_shutdown_must_drain_full_backlog_not_just_last_task` | `hayamimi/src/stream.rs` |
| `test_units.py` | `test_empty_specialist_falls_through_to_omni_when_installed` | `empty_specialist_falls_through_to_omni_when_installed` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_empty_specialist_stays_empty_on_a_minimal_install` | `empty_specialist_stays_empty_on_a_minimal_install` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_trim_lid_clip_skips_leading_quiet_preroll` | `trim_lid_clip_skips_leading_quiet_preroll` | `hayamimi-asr/src/lid.rs` |
| `test_units.py` | `test_trim_lid_clip_leaves_short_preroll_alone` | `trim_lid_clip_leaves_short_preroll_alone` | `hayamimi-asr/src/lid.rs` |
| `test_units.py` | `test_trim_lid_clip_caps_to_max_seconds` | `trim_lid_clip_caps_to_max_seconds` | `hayamimi-asr/src/lid.rs` |
| `test_units.py` | `test_sv_predicted_lang_matches_tag_substring` | `-` | 写さない：`eval_lid_candidates.py` は 3.2 が消す調査の道具 |
| `test_units.py` | `test_sv_predicted_lang_unknown_tag_is_question_mark` | `-` | 写さない：同上 |
| `test_units.py` | `test_target_clf_predict_picks_highest_probability_class` | `-` | 写さない：`lid_target_clf.py` は 3.2 が消す調査の道具 |
| `test_units.py` | `test_target_clf_predict_confidence_is_a_valid_probability` | `-` | 写さない：同上 |
| `test_units.py` | `test_verdict_for_rejects_when_any_criterion_fails` | `-` | 写さない：`eval_lid_candidates.py` は 3.2 が消す調査の道具 |
| `test_units.py` | `test_verdict_for_adopts_only_when_every_criterion_passes` | `-` | 写さない：同上 |
| `test_units.py` | `test_lid_max_seconds_is_the_documented_knob` | `lid_max_seconds_is_the_documented_knob` | `hayamimi-asr/src/lid.rs` |
| `test_units.py` | `test_tier_fallback_table_only_names_known_tiers` | `tier_fallback_table_only_names_known_tiers` | `hayamimi-asr/src/route.rs` |
| `test_units.py` | `test_punct4_prefers_int8_when_present` | `punct4_prefers_int8_when_present` | `hayamimi-asr/src/punct.rs` |
| `test_ws_ingest.py` | `test_compute_accept_key_matches_rfc_example` | `compute_accept_key_matches_rfc_example` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_parse_handshake_request_extracts_path_and_key` | `parse_handshake_request_extracts_path_and_key` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_parse_handshake_request_rejects_non_websocket` | `parse_handshake_request_rejects_non_websocket` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_parse_handshake_request_rejects_missing_key` | `parse_handshake_request_rejects_missing_key` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_build_handshake_response_contains_correct_accept` | `build_handshake_response_contains_correct_accept` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_build_handshake_request_roundtrips_through_parse` | `build_handshake_request_roundtrips_through_parse` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_frame_roundtrip_unmasked_binary` | `frame_roundtrip_unmasked_binary` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_frame_roundtrip_masked_text` | `frame_roundtrip_masked_text` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_frame_masked_bytes_differ_from_plaintext_on_wire` | `frame_masked_bytes_differ_from_plaintext_on_wire` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_decode_frame_incomplete_returns_none` | `decode_frame_incomplete_returns_none` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_decode_frame_two_frames_in_one_buffer` | `decode_frame_two_frames_in_one_buffer` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_decode_frame_large_payload_uses_64bit_length` | `decode_frame_large_payload_uses_64bit_length` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_frame_close_and_ping_opcodes_roundtrip` | `frame_close_and_ping_opcodes_roundtrip` | `hayamimi/src/ingest.rs` |
| `test_ws_ingest.py` | `test_decode_pcm16_mono_roundtrip` | `decode_pcm16_mono_roundtrip` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_decode_pcm16_drops_trailing_odd_byte` | `decode_pcm16_drops_trailing_odd_byte` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_decode_pcm16_averages_stereo_to_mono` | `decode_pcm16_averages_stereo_to_mono` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_decode_pcm16_empty_input` | `decode_pcm16_empty_input` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_resample_linear_identity_when_rates_match` | `resample_linear_identity_when_rates_match` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_resample_linear_upsamples_to_expected_length` | `resample_linear_upsamples_to_expected_length` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_resample_linear_downsamples_to_expected_length` | `resample_linear_downsamples_to_expected_length` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_resample_linear_preserves_a_ramp_shape` | `resample_linear_preserves_a_ramp_shape` | `hayamimi-asr/src/audio.rs` |
