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

各受入 ID（RB-1〜RB-8、RP-1〜RP-70、RP-72、RP-73、RP-79（呼ぶことの確かめの 1 行だけ。止めた、H130）、RP-80、RP-81、RP-83〜RP-87。RP-64 は欠番、RP-71 は 3.18 の段 3 の判定の後）の下の case 行（`T-RB-<n>-nnn`、`T-RP-<n>-nnn`）は、test-designer の席がこの節に書く。
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
| T-RP-3-022 | F88e(E2E の節の頭)を RP-3 の test に Rust の run として渡す | Rust の標本を閉じた規則が `early`。`L_asr − L_dec` = 200 ms | 標本ごとの線(early は 120 ms) | 有効 1(200 ≥ 120)。各 run で 有効 + final なし + 終端不正 の合計が枠と一致する(derived、H88 の「RP-3 も標本ごとの線」。`rp03.rs` は repo に無く、作る席がこの線で作る) | 境界、陽性 | `an_early_sample_of_200_ms_is_valid_for_the_rust_run`（未実装、`rp03.rs` を作る席が足す） |
| T-RP-3-023 | F88s を同じく渡す | 規則が `silence`。同じ 200 ms | 線 `min_silence`(300 ms) | 有効 0、終端不正 1(200 < 300)。022 と合わせて、全標本を 120 ms で数える build と、全標本を `min_silence` で数える build の両方が落ちる(derived、同上) | 境界、陰性 | `a_silence_sample_of_200_ms_is_bad_end_for_the_rust_run`（未実装、`rp03.rs` を作る席が足す） |

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
| T-RP-9-010 | 実走：Windows で `<hy> --input mix --serve` と `hayamimi-relay live` を 10 分、1 秒ごとに子孫の名前を集める | 実走 | 実走 | 集めた全部の名前に `python` を含む物が 0（specified、RP-9） | 陽性 | `rp09_no_python_at_run_time` |
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
| T-RP-10-007 | 実走：Windows 11、loopback へ英語の wav を鳴らし `/events` を読む | 実走 | 実走 | 001 の形（specified、RP-10）。マイクは入力の装置が在るときだけ同じ形 | 陽性 | `windows_loopback_reaches_events`（`tests/rp10.rs`、`#[cfg(windows)]` と `#[ignore]`。実走は窓 E4） |
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

test：`native/hayamimi/tests/rp16.rs`（001〜016、018〜022、035、038〜062）、`native/hayamimi-asr/tests/rp16_lid.rs`（023〜026）、`native/hayamimi-asr/tests/gpu_guard.rs`（027〜030）、`native/hayamimi-asr/tests/rp16_sessions.rs`（036）、`native/hayamimi-asr/tests/lid_dual.rs`（031〜034）、`native/hayamimi/src/server.rs`（017。src の中の `#[test]`）。受入 test の名前は `rp16_no_gpu_falls_back_to_cpu_and_says_so`（reason は expected_reason の値）。
`check_no_gpu_run(run, 理由)` は、終了 code、stdout の JSONL の event、`/events` の `fallback` の event、stderr の文を確かめる。`gpu.device.unavailable` の `reason` には、引数の理由を求める。
理由を決める関数 `expected_reason(起動)` は、`nvidia-smi -L` の起動の結果を受ける。終了 0 なら `no_device`、起動が `std::io::ErrorKind::NotFound`（PATH に無い）なら `driver_unreadable` を返す。終了 code が 0 以外、または `NotFound` 以外の起動の誤りは理由を返さず、終了 code か `ErrorKind` と path を名指す `Err` を返す。`hayamimi` の判定は呼ばない。2 つの値のどちらでも通す形は採らない。
fixture の行（001〜014、018〜022）は、fixture の中身が `no_device` なので `no_device` を渡し、機械に依らない。実物の `hayamimi` を起こす行（015、受入）は `expected_reason` の値を渡す。
run は、その run が読み込んだ 3.5 の GPU の段の一覧（R2 の ja の run では句読点、R3 以降は言語判定も）を持つ。落ちた event はこの一覧の段ごとに 1 件を求め、読み込んでいない段には求めない。
`check_gpu_run(run)` は GPU の在る run を確かめる。

訳の行（R8b）は、llama-server を起こす command（`hayamimi llama start --model <file> --port <n> --dir <root>`）の run を同じ形で確かめる（T-RP-16-038 から 051 と、理由の決め方の 052 から 062）。
受入 test の名前は `rp16_no_gpu_translate_server_falls_back_to_cpu_and_says_so`（GPU を隠した run を ja と C.UTF-8 の 2 つの言語で打ち、どちらも 038 の形（reason は expected_reason の値）で、stderr の言語が分かれる）。
`check_translate_on_cpu(run, 理由)` は、終了 code、stdout の JSONL の event、起こした server の引数、stderr の文を確かめる。
理由が `no_device` か `driver_unreadable` の run は `gpu.device.unavailable` を 1 件（`reason` = その理由）、`session_failed` の run は 0 件求める。実走の行（049、050）は `expected_reason` の値を渡し、fixture の行（038 から 048）は `no_device`（048 は `session_failed`）を渡す。
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
| T-RP-16-015 | 実走：`CUDA_VISIBLE_DEVICES` を空にして `<hy> --wav tests/golden/ja/ja_014.wav --no-realtime --mode single --lang ja` | 実走 | 実走 | 001 の形。ただし `reason` は `no_device` の固定でなく `expected_reason` が決めた値（driver の在る機械は `no_device`、`nvidia-smi` が PATH に無い機械は `driver_unreadable`）（specified、RP-16） | 陰性 | `hidden_gpu_falls_back_to_cpu_with_a_reason` |
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
| T-RP-16-037 | 実走：lock を外した build で、`rp16_lid -- --ignored --test-threads=2` を 20 回 | lock なしの対照 | 記録 | abort の回数を記録し、合否に使わない（derived。lock の根拠は、lock の無い形の 9 回で 2 回、lock を入れた後の同じ打ち方の 9 回で 0 回の実測である。abort は時刻の重なりで起きる稀な物で、lock を外した 3 つの構成の 20 回ずつでは 0 回だったので、再現しないことを lock が要らない証拠と読まない） | 記録 | `opening_sessions_without_the_lock_is_recorded`（未実装、持ち主 R13-16） |
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
| T-RP-16-049 | 実走：GPU を隠し（`HIDE_GPU`）、代役の server を入れた install に `hayamimi llama start --model ok.gguf --port <空き> --dir <install> --wait 20`（`LANG` = `C.UTF-8`） | 実走 | 実走 | 038 の形。ただし `reason` は `expected_reason` が決めた値。server は `/health` に `ok` を返す（specified、RP-16） | 陰性 | `hidden_gpu_translate_server_starts_on_cpu_with_a_reason` |
| T-RP-16-050 | 実走：049 と同じで、install に CUDA の build が無い（`llama/cuda` を作らない） | 起動の失敗が次の手へ落ちる | 実走 | 038 の形（`reason` は `expected_reason` が決めた値）に加えて `translate.server.failed` が 1 件（`build` = `cuda`、`reason` = `missing_build`）で、終了 code 0（specified、3.15「止めない」） | 境界、陰性 | `missing_cuda_build_falls_through_to_the_cpu_build` |
| T-RP-16-051 | 実走、GPU の在る機械：GPU を隠さず 049 と同じ command | 実走の対照 | 実走 | 046 の形（specified、RP-16 の対照） | 陽性 | `visible_gpu_translate_server_does_not_fall_back` |
| T-RP-16-052 | fixture：`nvidia-smi -L` の起動の結果が終了 0（driver の在る機械の形） | 決め方の陽性。終了 code の境界の 0 | `no_device` | `expected_reason` が `no_device`（specified、6 の RP-16 の行の決め方） | 境界、陽性 | `expected_reason_is_no_device_when_nvidia_smi_exits_zero` |
| T-RP-16-053 | fixture：起動の結果が `ErrorKind::NotFound`（PATH に無い。driver の無い機械の形） | 決め方の陽性（052 の対） | `driver_unreadable` | `expected_reason` が `driver_unreadable`。052 と同じ値にならない（specified、同上） | 陽性、陰性 | `expected_reason_is_driver_unreadable_when_nvidia_smi_is_not_found` |
| T-RP-16-054 | fixture：起動の結果が終了 code 1（`nvidia-smi` は在るが 0 以外で終わる） | 終了 code の境界の 1。期待を推し量らない | `Err` | `Err` で終了 code 1 を名指し。`no_device` も `driver_unreadable` も返さない（specified、同上） | 境界、陰性 | `expected_reason_names_a_nonzero_nvidia_smi_exit` |
| T-RP-16-055 | fixture：起動の結果が `ErrorKind::PermissionDenied`（path `/usr/bin/nvidia-smi`） | `NotFound` の隣の誤り（権限が無い） | `Err` | `Err` で `PermissionDenied` と path を名指し。`driver_unreadable` を返さない（specified、同上） | 境界、陰性 | `expected_reason_names_a_permission_denied_launch_and_its_path` |
| T-RP-16-056 | fixture：起動の結果が `ErrorKind::Other`（path `/usr/bin/nvidia-smi`。実行できない形式などの誤りの形） | `NotFound` 以外の起動の誤りの別の種類 | `Err` | `Err` で `Other` と path を名指し（specified、同上） | 陰性 | `expected_reason_names_any_other_launch_error_kind` |
| T-RP-16-057 | fixture：`gpu.device.unavailable` の `reason` が `driver_unreadable` で、001 の他の形が揃った run を、理由 `driver_unreadable` で確かめる | driver の無い機械の wav の実走の形 | `Ok` | `check_no_gpu_run` が `Ok`（specified、6 の RP-16 の行） | 陽性 | `no_gpu_run_with_driver_unreadable_reason_passes_when_expected` |
| T-RP-16-058 | fixture：057 と同じ run を、理由 `no_device` で確かめる | driver の在る機械で `driver_unreadable` を出す build | `Err` | `Err` で `driver_unreadable` と `no_device` の両方を名指し（specified、同上。2 つのどちらでも通す形を採らない） | 陰性 | `driver_unreadable_reason_is_rejected_when_the_driver_is_present` |
| T-RP-16-059 | fixture：001 の run（`reason` = `no_device`）を、理由 `driver_unreadable` で確かめる | driver の無い機械で `no_device` を出す build | `Err` | `Err` で `no_device` と `driver_unreadable` の両方を名指し（specified、同上） | 陰性 | `no_device_reason_is_rejected_when_the_driver_is_absent` |
| T-RP-16-060 | fixture：038 と同じ run で `gpu.device.unavailable` の `reason` が `driver_unreadable`。理由 `driver_unreadable` で `check_translate_on_cpu` | 訳の server の行の driver の無い機械の形 | `Ok` | `Ok`（specified、同上） | 陽性 | `translate_run_with_driver_unreadable_reason_passes_when_expected` |
| T-RP-16-061 | fixture：060 と同じ run を、理由 `no_device` で確かめる | 訳の server の行で理由が食い違う | `Err` | `Err` で両方の値を名指し（specified、同上） | 陰性 | `translate_run_with_the_wrong_device_reason_is_rejected` |
| T-RP-16-062 | fixture：038 と同じ run（`gpu.device.unavailable` の `reason` = `no_device`）を、理由 `driver_unreadable` で `check_translate_on_cpu` に確かめる | driver の無い機械で `no_device` を出す build（061 の逆向き。059 の訳の server 版） | `Err` | `Err` で `no_device` と `driver_unreadable` の両方を名指し（specified、6 の RP-16 の行。2 つのどちらでも通す形を採らない） | 陰性 | `translate_run_with_no_device_reason_is_rejected_when_the_driver_is_absent` |

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
`check_port_table(table, listed_names)` は、5 の対応表の Rust の test の名前を `cargo test --release -- --list` の出力と突き合わせる。表で Rust の名前が `-` の行（写さない物）は、置き場の列に理由が在ることを確かめる。理由は 5 種で、5 つ目の `写さない：Python の口（<Rust の在り処、または持たない機構>）` は括弧の中が空でないことまで確かめる（5.2）。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp19`。`--ignored` を付ける行は 010 だけである。

R11 の前の後退の検出（010 より後の行）は、次の 2 つの関数で受ける。
`check_not_yet_set(table_names, listed_names, fixed, ported, closed)` は、表の名前を持つ行のうち list に無い行の集合（未実装の行）が、固定の一覧 `fixed`（`rp19_not_yet_at_r11.tsv` の名前と持ち主の札の対）から、`ported`（`rp19_ported/` の各 file の名前と、その file が示す持ち主の札の対）の全名前と、`closed`（5.3 で Rust の名前が `-` の行の名前。case の名前から先頭の `test_` を除いた物）の全名前を引いた集合とちょうど同じであること、と、`ported` の各名前が固定の一覧でその file の持ち主の札の行であって、`closed` の名前でないことを確かめる。
file 名の読みは、`.txt` を落とし、最初の `.` より前を持ち主の札とする。札を丸ごと持つ file は `<札>.txt`、札を複数の R13 の slice が分けて持つ file は `<札>.<R13 の slice>.txt` である。持ち主の検査は札の単位で行い、R13 の slice の綴りは読まない。
`check_fixed_list(tsv_text, pinned_sha256, pinned_rows)` は、固定の一覧の内容の sha256 と行の数が 2 つの定数と同じであることを確かめる。
壊した入力は `native/hayamimi-eval/tests/fixtures/rp19_r11/` の file に置いて `include_str!` で渡す。基準の fixture は、表の名前が `alpha`（持ち主 `R12b`）、`beta`、`gamma`（どちらも持ち主 `R12e`）の 3 行、固定の一覧がその 3 行、list が `alpha` だけ、`rp19_ported/R12b.txt` が `alpha` だけの状態（基準の状態）で、`ported` を作る関数は無い dir を空として読む。
基準の状態で `closed` は空で、028 から 030 だけが `beta` を閉じた名前として渡す（表の名前は {alpha, gamma}）。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。例外は 2 行で、010 は `rp19_` を付けた `rp19_python_cases_are_ported`、025 は受入 test の名前の全部である。
残る穴：同じ名前を `R12b.R13-1.txt` と `R12b.R13-2.txt` の 2 つに書いても、`set_errors` が `ported` の名前を `BTreeSet` に集めるので重複は 1 つになり、検出しない。期待値の無い行は関数の末尾が 1 対 1 の決まりを壊すので、case 行にしない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-19-001 | 5 の表の行数と、`tests/*.py` の case の数 | 全数 | 一致 | 表の行数 = 386。数は `git ls-files 'tests/*.py' \| xargs grep -c '^def test_\|^    def test_'` の合計（pseudo、2026-10-13 に打った値） | 陽性 | `table_row_count_equals_python_case_count` |
| T-RP-19-002 | fixture：表の Rust の名前が全部 list に在る | 突き合わせ | `Ok` | `Ok`（specified、RP-19） | 陽性 | `table_names_all_listed_pass` |
| T-RP-19-003 | fixture：表の 1 行目の名前（対応表の 1 行目）が list に在る | 陽性対照 | `Ok` | 1 行目の名前を含む list で `Ok`（specified、RP-19 の陽性対照） | 陽性 | `first_table_row_name_is_found` |
| T-RP-19-004 | fixture：list の名前を 1 つ変えた物 | 名前の食い違い | `Err` | `Err` でその名前を名指し（specified、RP-19 の陰性欄） | 陰性 | `renamed_test_in_list_is_named` |
| T-RP-19-005 | fixture：表の 2 行が同じ Rust の名前 | 重複 | `Err` | `Err` でその名前を名指し（derived、1 つの名前が 2 つの case を持つと、片方が消えても気付けない） | 境界、陰性 | `duplicate_rust_name_in_table_is_rejected` |
| T-RP-19-006 | fixture：Rust の名前が `-` で置き場の列が空の行 | 写さない行の理由 | `Err` | `Err` でその行の Python の case を名指し（derived、理由の無い「写さない」を許さない） | 陰性 | `unported_row_without_reason_is_rejected` |
| T-RP-19-007 | fixture：Rust の名前が `-` で置き場の列に `写さない：<理由>`（5.2 の最初の 4 種のどれか）の理由が在る行 | 写さない行 | `Ok` | `Ok`（derived）。5 つ目の種類は 011 から 013 が持つ | 境界 | `unported_row_with_reason_is_accepted` |
| T-RP-19-008 | `docs/spec/ja_pipeline_spec.json` の `config` と `<hy> config dump` から作り直した値 | 値の一致 | 差 0 | 差 0（specified、RP-19） | 陽性 | `ja_pipeline_spec_config_matches_dump` |
| T-RP-19-009 | fixture：dump の 1 つの値を変えた物 | 値の食い違い | `Err` | `Err` でその key の path を名指し（specified、RP-19） | 陰性 | `changed_config_value_is_named` |
| T-RP-19-010 | 実走：`cargo test --release -- --list` の実物と 5 の表 | 実走 | 実走 | 001 から 007 と 011 から 013 の判定を実物で通し、表の全行の Rust の test が全部通る（specified、RP-19）。未実装の行が 1 行でも在る間は落ち、slice ごとの数を出す（8 の「RP-19 の未実装 187 行の仕分けと slice」） | 陽性 | `rp19_python_cases_are_ported` |
| T-RP-19-011 | fixture：Rust の名前が `-` で、置き場の列が `写さない：Python の口` だけの行 | 5 つ目の種類。見出しの語だけで、在り処も機構も無い | `Err` | `Err` でその行の Python の case を名指し（specified、RP-19 の「在り処も機構も無い行は許さない」。8 の「写さない行」の「ラベルだけで在り処も機構も無い行は (a) か (c) に戻す」） | 陰性 | `python_mouth_row_without_place_or_mechanism_is_rejected` |
| T-RP-19-012 | fixture：置き場の列が `写さない：Python の口（）` の行 | 括弧は在るが中が空。011 の隣の境界 | `Err` | `Err` でその行の Python の case を名指し（specified、同上。括弧の記号だけでは在り処にならない） | 境界、陰性 | `python_mouth_row_with_empty_parentheses_is_rejected` |
| T-RP-19-013 | fixture：置き場の列の括弧の中に、機構の文（5.3 の `run_stream_without_control_is_unaffected` の行と同じ文）が在る行 | 括弧の中が空でない。012 の 1 字先 | `Ok` | `Ok`（specified、RP-19 の 5 つ目の種類） | 境界、陽性 | `python_mouth_row_with_a_mechanism_is_accepted` |
| T-RP-19-014 | 基準の状態から list の `alpha` を除く（`alpha` は `rp19_ported/R12b.txt` に在る。写した後で test を消した形） | 写した後に消した。`ported` に在って list に無い名前 | `Err` | `Err` で `alpha` を名指す。表と list から数えた未実装の集合 {alpha, beta, gamma} が、固定の一覧 − ported の {beta, gamma} と違う（specified、8 の RP-19 の小節の 1 と、7 の RP-19（R12a1）の陰性の case「写した後に消した」） | 陰性 | `a_name_ported_and_then_deleted_from_the_list_is_rejected` |
| T-RP-19-015 | 基準の状態から `rp19_ported/R12b.txt` を空にする（`alpha` は list に在って通る） | 写した名前を `rp19_ported/` に書かない。list に在って `ported` に無い名前 | `Err` | `Err` で `alpha` を名指す。表と list から数えた未実装の集合 {beta, gamma} が、固定の一覧 − ported の {alpha, beta, gamma} と違う（specified、同小節の 1 と、7 の陰性の case「写した名前を `rp19_ported/` に書かない」） | 陰性 | `a_ported_name_missing_from_the_ported_files_is_rejected` |
| T-RP-19-016 | 基準の状態の固定の一覧に、`delta` と持ち主 `R12e` の行を足した物。定数は基準の一覧の sha256 と 3 のまま | 一覧に行を足した。行の数 | `Err` | `Err` で行の数 4 と 3 の食い違いと、sha256 の食い違いを言う（specified、同小節の 3 と、7 の陰性の case「`rp19_not_yet_at_r11.tsv` に行を足した」） | 陰性 | `a_row_added_to_the_fixed_list_is_rejected` |
| T-RP-19-017 | 基準の状態で `rp19_ported/R12e.txt` に `alpha` を書く（固定の一覧で `alpha` の持ち主は `R12b`）。`R12b.txt` は空にする | ほかの持ち主の札の行を自分の file に書いた。`ported` の和は {alpha} で、検査 1 は通る形 | `Err` | `Err` で file `R12e.txt`、名前 `alpha`、持ち主 `R12b` を名指す（specified、同小節の 2 と、8 の「R11 までの組」の G0 の行の陰性の case「ほかの札の行を自分の file に書いた」。検査 1 だけでは通るので、検査 2 を単独で測る） | 陰性 | `a_name_written_in_another_slices_file_is_rejected` |
| T-RP-19-018 | 基準の状態で `rp19_ported/R12b.txt` に `zeta`（固定の一覧に無い名前）を足す | 名前を変えた、または一覧に無い名前。検査 2 の持ち主が引けない側 | `Err` | `Err` で file `R12b.txt` と名前 `zeta` を名指す（derived、固定の一覧の外の名前はどの slice の行でもない。017 の隣の同値類） | 境界、陰性 | `a_name_unknown_to_the_fixed_list_is_rejected_in_a_ported_file` |
| T-RP-19-019 | 基準の状態の固定の一覧で、`gamma` の持ち主の札の綴りを 1 字変えた物（行の数は 3 のまま） | 行の数は同じで内容が違う。sha256 | `Err` | `Err` で sha256 の食い違いを言い、行の数の食い違いは言わない（specified、同小節の 3。行の数だけを見る検査はこの入力を通す） | 境界、陰性 | `a_changed_row_with_the_same_count_changes_the_digest` |
| T-RP-19-020 | 基準の状態そのまま（`alpha` は写されて list に在り `R12b.txt` に書かれ、`beta` と `gamma` は未実装） | 3 つの検査が全部通る状態 | `Ok` | `Ok`。未実装の集合が {beta, gamma}（specified、同小節。014 から 019 の陰性の対） | 陽性 | `the_baseline_state_passes` |
| T-RP-19-021 | 基準の状態から `rp19_ported/` を取り除き、list を空にする | 写した名前が 0 件。`ported` の dir が無い | `Ok` | `Ok`。未実装の集合が固定の一覧と同じ {alpha, beta, gamma}（specified、8 の R12a1 の行「無い dir は空として読む」） | 境界 | `an_absent_ported_directory_reads_as_empty` |
| T-RP-19-022 | list に 3 つとも在り、`R12b.txt` に `alpha`、`R12e.txt` に `beta` と `gamma` を書く | 未実装の行が 0 | `Ok` | `Ok`。未実装の集合が空で、固定の一覧 − ported も空（specified、同小節。`rp19_python_cases_are_ported` が通る状態） | 境界 | `a_fully_ported_state_passes` |
| T-RP-19-023 | 5.3 の実物（この file の 5.3） | Python の口の行の数 | 20 | `写さない：Python の口（` で始まり括弧の中が空でない行がちょうど 20（specified、8 の「写さない行」の 15 行と、5.2 の `test_ws_ingest.py` の server 側の handshake の 5 行）。pseudo：`sed -n '/^### 5.3/,$p' docs/design/rust_port_tests.ja.md \| rg -c '\| \x60-\x60 \| 写さない：Python の口（.'` が 20（実測 2026-10-22）。陽性対照：式の `Python の口（.` を `同上` に替えると 39 で、式が行に当たる（0 は「式が合わない」と「行が無い」を分けない） | 陽性 | `the_table_has_exactly_twenty_python_mouth_rows` |
| T-RP-19-024 | 5.3 の実物の `test_ws_ingest.py` の 21 行 | `/ingest` の codec の置き場 | 8、4、4、5 | 置き場の列がちょうど `hayamimi-eval/src/ws.rs` の行が 8、ちょうど `hayamimi-asr/src/audio.rs` の行が 4、ちょうど `hayamimi/src/capture/convert.rs` の行が 4、`写さない：Python の口（` の行が 5、ちょうど `hayamimi/src/ingest.rs` の行が 0（specified、5.2 の `hayamimi-eval/src/ws.rs` の項）。pseudo：`sed -n '/^### 5.3/,$p' docs/design/rust_port_tests.ja.md \| rg '^\| \x60test_ws_ingest\.py\x60 \|' \| rg -c '\| \x60hayamimi-eval/src/ws\.rs\x60 \|$'` が 8、末尾の式を `\| \x60hayamimi-asr/src/audio\.rs\x60 \|$` に替えると 4、`\| \x60hayamimi/src/capture/convert\.rs\x60 \|$` に替えると 4、`\| \x60-\x60 \| 写さない：Python の口（.` に替えると 5、`\| \x60hayamimi/src/ingest\.rs\x60 \|$` に替えると出力なしで rc 1（実測 2026-10-22）。置き場の列で数えるのは、閉じた 5 行の括弧の中が `hayamimi-eval/src/ws.rs` と `hayamimi/src/ingest.rs` を名指すからである。4 つの和 21 が file の行の数と同じことが陽性対照 | 陽性、境界 | `the_ingest_codec_rows_are_placed_in_eval_ws_rs` |
| T-RP-19-025 | 受入：merge の直前の main の `native/hayamimi-eval/tests/rp19_not_yet_at_r11.tsv`、`rp19_ported/`、5.3、`--list` の実物 | 全体。検査 1 から 3 | `Ok` | `Ok`。未実装の集合が固定の一覧 − ported − 5.3 で閉じた名前と同じ（specified、RP-19 と 5.2）。pseudo：`wc -l < native/hayamimi-eval/tests/rp19_not_yet_at_r11.tsv` が 172 で、`sha256sum` の値が `NOT_YET_AT_R11_SHA256` と同じ。陽性対照は 020 と 028、陰性対照は 014 から 019 の 6 行と 029、030 | 陽性、陰性 | `rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks` |
| T-RP-19-026 | 基準の状態の `rp19_ported/R12b.txt` を `rp19_ported/R12b.R13-1.txt` に替える（中身は `alpha` のまま。固定の一覧で `alpha` の持ち主は `R12b`） | 札を複数の R13 の slice が分けて持つ file 名 `<札>.<R13 の slice>.txt`。`.txt` を落とした `R12b.R13-1` の最初の `.` より前が持ち主の札 | `Ok` | `Ok`。`ported` の和は {alpha}、file `R12b.R13-1.txt` が示す持ち主の札 `R12b` は、固定の一覧で `alpha` の持ち主と同じ。未実装の集合は {beta, gamma}（specified、file 名の読み「`.txt` を落とし、最初の `.` より前を持ち主の札とする」） | 陽性、境界 | `a_file_named_owner_dot_slice_reads_the_owner_before_the_first_dot` |
| T-RP-19-027 | 基準の状態の `rp19_ported/R12b.txt` を `rp19_ported/R12e.R13-1.txt` に替える（中身は `alpha` のまま。固定の一覧で `alpha` の持ち主は `R12b`） | 026 と同じ綴りで札だけが違う。最初の `.` より前の `R12e` が持ち主の札で、`alpha` の持ち主 `R12b` と違う。`ported` の和は {alpha} で、検査 1 は通る形 | `Err` | `Err`（`OtherSlice`）で file `R12e.R13-1.txt`、名前 `alpha`、持ち主 `R12b` を名指す（specified、同上。R13 の slice の綴りが 026 と同じでも、札が違えば落ちる。持ち主の検査が札の単位であることを、026 と 1 字違いで測る隣の陰性） | 陰性 | `a_name_in_a_file_of_another_owner_with_a_slice_suffix_is_rejected` |
| T-RP-19-028 | 基準の状態から、表の `beta` の行を 5.3 で `-`（置き場 `写さない：Python の口（…）`）にする。表の名前は {alpha, gamma}、閉じた名前は {beta}。固定の一覧、list（`alpha`）、`R12b.txt`（`alpha`）は基準のまま | 固定の一覧に在り、`ported` に無く、表で閉じた名前。表の名前に入らないので、list に無い行の側にも入らない | `Ok` | `Ok`。表と list から数えた未実装の集合が {gamma}、固定の一覧 − ported − 閉じた名前も {gamma}（specified、5.2 の「期待値は、固定の一覧の名前から、`rp19_ported/` の名前と、この表で Rust の名前が `-` の行の名前を引いた集合」）。029 が閉じた名前だけを渡さない隣の陰性 | 陽性 | `a_name_closed_in_the_table_is_left_out_of_the_expected_not_yet_set` |
| T-RP-19-029 | 028 と同じ状態（表の名前は {alpha, gamma}、`beta` の行は 5.3 で `-`）で、`closed` に空の集合を渡す（閉じた名前を引かない build の読み） | 固定の一覧に在り、`ported` に無く、閉じた名前として渡されない名前。028 と `closed` の 1 項だけが違う | `Err` | `Err` の 1 件で `ListedNotPorted` が `beta` を名指す。未実装の集合は表と list から {gamma}、固定の一覧 − ported は {beta, gamma} なので、差の `beta` が残る（specified、同上。閉じた名前の項が固定の一覧の名前を 1 つだけ消していることを、028 と 1 項違いで測る） | 陰性 | `a_closed_name_that_is_not_subtracted_is_named_as_listed_not_ported` |
| T-RP-19-030 | 028 の状態（`beta` が 5.3 で `-`、閉じた名前 {beta}）で、`rp19_ported/R12e.txt` に `beta` だけを書く（固定の一覧で `beta` の持ち主は `R12e`。fixture `fixtures/rp19_r11/ported_beta.txt` は作る）。list は `alpha`、`R12b.txt` は `alpha` のまま | 写したのに閉じた。閉じた名前が `ported` にも在る。未実装の集合 {gamma} と固定の一覧 − ported − 閉じた名前 {gamma} が一致し、持ち主の札も合うので、集合の検査と持ち主の検査は通る形 | `Err` | `Err` の 1 件で `ClosedAlsoPorted` が file `R12e.txt` と名前 `beta` を名指す（specified、`check_not_yet_set` の 2 つめの確かめ「`ported` の名前は `closed` の名前でない」）。落ちるのは、`ported` の file ごとに名前を引く検査（持ち主の検査と同じ走査）が `closed` と `ported` の共通部分を見るからで、`--list` の出力に `beta` が在るかは結果を変えない。閉じた行は Rust の名前を持たず表の名前に入らないので、`PortedNotListed`（表の名前が list に無い行）の経路には掛からない | 陰性 | `a_name_both_closed_in_the_table_and_ported_is_rejected` |

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

test：`native/hayamimi-eval/tests/rp22.rs`（001〜006）と `native/hayamimi/tests/rp22.rs`（007〜011）。
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
| T-RP-29-011 | fixture：port 8833 を塞いだ run で、実際の port が 8843、`server.fallback.port` 1 件（`reason` = `in_use`）、出した URL が実際の port。port 65533 を塞いだ時の実際の port が 65535 と 65533 | 次の番号から 10 個の内側（最後）。port の上限で 10 個に満たない時の最後 | `Ok`。65533 の時の 65533 は `Err` | `Ok`。8834 から 8843 の 10 個（derived、3.15 の「次の番号から 10 個まで」を数えた値）。65533 の次の番号は 65534 と 65535 の 2 個で、65535 が最後（derived、port の上限 65535） | 境界、陰性 | `blocked_port_uses_the_tenth_next_port` |
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
| T-RP-29-029 | `native/hayamimi/src/run.rs` の `mod tests`（`rp29.rs` ではない）：清書の thread を起こせない starter を渡した run と、起こせる starter を渡した run を、同じ test の中で 1 回ずつ | 清書の thread を起こせない場面（3.15 の `refine`） | 起こせない側は `None`（`start_refiner` は他に失敗しないので `Result` を返さない）と notice が 1 つ。起こせる側は notice が 0 | 起こせない側は run を止めず `refiner: None` で続け、notice の `stage` = `refine`、`reason` = `spawn_failed`、`to` = `disabled` の 1 つが `report::say_notice` に渡る。起こせる側は notice が 0（specified、R13-20b の (a)、3.15 の表の清書の行） | 陰性、陽性 | `a_refiner_that_cannot_start_falls_back_and_the_run_goes_on` |
| T-RP-29-030 | `native/hayamimi/src/run.rs` の `mod tests`：清書の句読点の `restore` が `Err` を返す台本の文と、`Ok` を返す台本の文を、同じ test の中で 1 回ずつ | 清書の句読点が失敗した文（live の `asr.punct.failed` と同じ形） | 失敗側は `refine.punct.failed` が 1 回。成功側は 0 回 | 失敗側は文が句読点なしのまま変わらず、log に warn の `refine.punct.failed` が 1 行（component `asr`、error_code なし、group の `audio_ms` を持つ）。成功側は 0 行（specified、R13-20b の (b)） | 陰性、陽性 | `a_failed_refine_punctuation_is_said_as_an_event` |
| T-RP-29-031 | `native/hayamimi/src/run.rs` の `mod tests`：清書の句読点を開く関数が `Err(AsrError::MissingModel)` を返す run（group を 2 つ流す。live の句読点の `fallback:punct` が先に取ってある）と、開ける関数を渡した run | 清書の句読点の model が無い場面（3.15 の `refine_punct`）、1 回だけ言う側の境界（2 つ目の group） | 無い側は `fallback` が 1 つ（2 つ目の group の後も 1 つ）。開ける側は 0 | 無い側は `fallback` の `stage` = `refine_punct`、`reason` = `missing`、`to` = `disabled` が 1 つで、2 つ目の group で増えず、取ってある live の `fallback:punct` が消えずに残る。開ける側は `fallback` が 0（specified、R13-20b の (c)） | 陰性、陽性、境界 | `an_unavailable_refine_punctuation_sends_one_fallback_event` |
| T-RP-29-032 | `native/hayamimi/src/run.rs` の `mod tests`：呼ばれた `Device` を記録する偽の開く関数を、清書の句読点を開く 1 か所の組み立てへ渡す。陰性は、組み立ての `Device::Cpu` を `Device::Cuda` に替えた build でこの test が落ちることを、手で 1 回確かめる | 清書の句読点を開く device（CPU に固定、10 の H147） | 記録された列が `[Device::Cpu]`（1 回呼ばれ、その 1 回が CPU） | `[Device::Cpu]` と等しい。空の記録は通さない（呼ばれないことを合格にすると何も測らない）。手で替えた build では落ちる（specified、R13-20b の (j)、H147。陰性の確かめは human） | 陽性、陰性 | `the_refine_punctuator_is_opened_on_the_cpu` |

### 4.40 RP-30（`fallback` の `stage` と `reason` の値の一覧が code と同じ）

test：`native/hayamimi/tests/rp30.rs`。受入 test の名前は `rp30_fallback_stage_and_reason_lists_match_the_code`。`#[ignore]` を持たない。
`check_fallback_lists(design, code)` は、`docs/design/rust_port.ja.md` の 3.15 の「`fallback` の event の `stage` の値は」の段落の 2 つの一覧と、`native/hayamimi/src/notice.rs` の `STAGES` と `REASONS` を集合として比べ、どちらかにだけ在る値を名指しして `Err` を返す。
fixture の docs は、実物の段落を file に写して 1 語だけ直した物を `native/hayamimi/tests/rp30/` に置く（反例を command や test の本文の文字列で運ばない）。
実物の読みは、`docs/design/rust_port.ja.md` と `native/hayamimi/src/notice.rs` をそのまま渡す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-30-001 | 実物の `rust_port.ja.md` と実物の `notice.rs` | 一致 | `Ok` | `Ok`（specified、RP-30 の「集合として code と一致する」） | 陽性 | `design_lists_equal_the_code_constants` |
| T-RP-30-002 | 実物の段落から読んだ値の数 | 読めていること | `stage` が 15 個、`reason` が 12 個 | 15 と 12（specified、RP-30 の陽性対照。001 の `Ok` が空と空の一致ではないことを示す） | 陽性 | `the_real_paragraph_yields_15_stages_and_12_reasons` |
| T-RP-30-003 | 実物の段落。括弧の中の `unreachable`、`unsupported_target` と、中継の `sink` | `hayamimi` の一覧の外の値 | `reason` の集合にこの 2 つが無い。`stage` の集合に `sink` が無い | 無い（specified、3.15 の「`hayamimi` が 12 個」と括弧書き。段落の読み方が括弧の中まで拾うと 002 の 12 が 14 になって落ちる） | 境界、陰性 | `values_named_for_the_relay_are_not_read_as_hayamimi_values` |
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
| T-RP-32-014 | 001 で 4 番目の run の前の load の行が `1.99 0.50 0.50 1/300 1000`、後の行が `9.99 0.50 0.50 1/300 1000`（後の 1 分の欄だけが高い） | 後の 1 分の load。run 自身が押し上げる欄 | `Ok` | `Ok`。後の 1 分の load は記録だけで判定しない（specified、RP-83 の (1) が後の欄で見るのは 5 分平均だけ。run 自身が 1 分の値を押し上げるので、3.19 の「E2E の run の門」は後の値に 5 分平均を使う）。出力に後の 1 分の値 999 が残る。後の 5 分平均と前の 15 分平均の 2.50 の線と、run の間の `cargo` と `rustc` は RP-83 が判定する（T-RP-83-005、007、009）。この行は RP-83 と矛盾しない | 境界 | `the_one_minute_load_after_a_run_is_recorded_but_not_judged` |
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
| T-RP-32-029 | 実走：Python、Rust、Python、Rust、Python、Rust の 6 run と、各 run の前後の `/proc/loadavg` | 実走 | 実走 | 001 の形で `Ok`（specified、RP-32）。この実走が読む `docs/results/rp32/` の Rust の `rs1`〜`rs3` は `<run>.machine.jsonl` を持たない門の前の記録で、3.19 の「門の前の記録」の一覧（dir、run の名前、`<run>.jsonl` の sha256）に当たるので、RP-83 の判定から外れる。実走の出力に、3 run を門の前の記録として外したことが在る（specified、RP-83 の (6)）。RP-32 の最後の判定（窓 E4）は、門の後に流した新しい run で出す | 陽性 | `rp32_alternated_runs_under_the_same_load_beat_python` |
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
| T-RP-32-047 | 001 の 6 run のうち Rust の 3 run が、T-RP-83-001 の形の `<run>.machine.jsonl` を持つ。2 番目の Rust の run（4 番目）の行 1 の `load15` を `"2.50"` にする | E2E の run の門を呼ぶ確かめ（RP-83 の線の値） | `load15` < 2.50 | `Err` で 4 番目の run の名前、`load15`、`2.50` を名指す。ほかの 2 つの Rust の run の名前は名指さない（specified、3.19 の「E2E の run の門」の 5「呼ぶことの確かめ」。support を呼び忘れた判定は `Ok` を返して、ここで落ちる。3 run 全部が内側の形が陽性対照）。fixture の 3 run は門の前の記録の一覧に当たらない一時 dir に置く | 陰性 | `a_rust_run_with_a_first_row_load15_of_2_50_is_rejected_by_the_gate` |
| T-RP-32-048 | 001 の Rust の run で、1 つの標本の `t_final_us` に、同じ `t_us` を持つ events の `final` を 2 つ置く(`id` は `mic-3` と `mic-4`)。stdout の `decided` は `id` 3 が `early`、`id` 4 が `silence` | 規則の違う 2 つの `final` が同じ `t_us` に在る標本 | 標本を閉じた規則が 1 つに決まる | `Err` で run の名前と `mic-3`、`mic-4` を名指す。線を選ばず、`Ok` にもしない(specified、3.1 の終端不正の例外「同じ `t_us` に規則の違う `final` が 2 つ在る標本は、run と `id` を名指して落とす」。`rp32_rules.rs` の `closing_rule` の断りを受ける行。2 つとも `early` なら線は 1 つに決まって `Ok` になることが陽性対照で、T-E2E-1-021 が持つ) | 境界、陰性 | `two_finals_with_different_rules_at_one_t_us_are_rejected_naming_both_ids` |
| T-RP-32-046 | 037 の出力から、訳の取れた割合の列を除いた出力 | 列の欠け | `Err` | `Err` で訳の取れた割合の列を名指す。Rust の割合の列も Python の割合の列も、設計が挙げる列である（specified、RP-32 の「別の列に出す」。024 と 025 の形。列の見出しの綴りは実装の席が決め、test はその綴りを 1 か所に持つ） | 陰性 | `a_missing_translated_share_column_is_rejected` |

**T-RP-32-037 から 046 は未実装。実装の席が足す**（D 3.18 の「測り方の直し」の順 1。製品の code は変えない）。10 本とも fixture を読み、`--ignored` を付けずに通る。`check_alternated_runs` に `check_translated_share`（`native/hayamimi-eval/src/compare.rs`）を Rust の 3 run を合わせた標本へ掛ける 1 行と割合の列が入るまで、今の main で落ちるのは 038（`Ok` になる）、039（載る理由が 3 つで割合が無い）、042、045（割合の列が無い）、044（`Ok` になる）、046（列を調べる関数が無い）である。037、040、041、043 は割合を見ない今の main でも通り、割合を足した後で通ったままであることを固定する側である。
**割合の計算は整数で行う**：`訳の取れた × 23 ≥ 有効 × 20`（`compare.rs` の `SHARE_DENOMINATOR` と `SHARE_NUMERATOR`）。小数の割合を作らない。`compare.rs` の unit test `twenty_of_twenty_three_have_enough_translations_and_nineteen_do_not`（23 標本の 20 と 19）が、この関数の単体の陽性対照で、この節の行は関数を run の判定に掛けた後の値を固定する。
**F32L の作り方**：標本は式で作る。Python の k 番目の run の発話 u の repeat r の標本は `L` 1000 ms、`t_end` 0、`L_asr` 700 ms、復号 300 ms（無音 400 ms）。Rust も同じで、訳の取れない標本は `t_tr` を持たない。実測のふりをしない。

**T-RP-32-030 から 036 は未実装。実装の席が feat/rp32-test に足す。** 7 本とも fixture の dir を読み、`--ignored` を付けずに通る。`pairs.txt` の中身の例は `2` と改行 1 つ（D 3.16 の 4 の「算用数字 1 つを 1 行で」）。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 「訳」の段（`final` から `t_tr` まで）と `L_tr`（`t_tr − t_final`）は、3.1 の定義では同じ値である。設計が 2 つを別の列に挙げる意図が、別の測り方（たとえば中継の log の時刻）なら、その測り方。023 は同じ値を 2 列に出す形で書いた。
- 対の比較の「発話ごとの 5 回の中央値」は、3 対を合わせると 1 発話 15 標本になる。F32 は 1 発話 3 標本（1 回 × 3 対）で、中央値を 3 つから取る。比較集合の「Python の全 run」を 3 つの run のそれぞれと読んだ（021）。
- 後の 1 分の load は記録だけにした（014）。後の 5 分平均の 2.50 未満と前の 15 分平均の 2.50 未満は RP-83 の判定で、4.75 の T-RP-83-004 から 007 が持つ。RP-83 の実装が入った後は、F32 の Rust の run が `<run>.machine.jsonl`（F83 の形）を持つ形にする。持たないと Rust の run が「file の無い run」で落ち、001 から 046 が通らなくなる（R8x-1 の席が `f32_runs()` に足す）。
- 「他の席が cargo の build を打たない」ことは、前の 1 分の load が 2 未満であることでは見えない。build が run の途中に始まっても、前の load には出ない。run の間の `cargo` と `rustc` の数は RP-83 が `<run>.machine.jsonl` の全行で見る（4.75 の T-RP-83-009 から 013）。
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
| T-RP-33-016 | 実走：RP-32 の Rust の run の `/events`（B の後の run）。A を作った後は A の前の run も | 実走 | 実走 | 001 の形で、全行の「訳」の段が Python 以下（specified、RP-33）。この実走が読む `docs/results/rp32/` の Rust の `rs1`〜`rs3` は、門の前の記録の一覧に当たる run で、RP-83 の判定から外れる。出力に、外したことが在る（specified、RP-83 の (6)） | 陽性 | `rp33_first_translation_after_a_failed_draft_beats_python` |

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
入力：`tests/fixtures/e2e_columns.jsonl` は 4 件の標本（`t_end_us` は全部 1000000）。`t_final_us` と `t_tr_us` と `final_latency_ms`（`L_dec`）は 1 件目が 2300000、2500000、1000.0。2 件目が 2300000、2400000、1000.5。3 件目が 2000000、3000000、100.0。4 件目が 1800000、null、100.0。`--min-silence` は 300 ms。有効は `L_asr − L_dec ≥` その標本の線（3.1 の分割式と標本ごとの線、H88）で、`L_asr = t_final − t_end`、`L = t_tr − t_end`、`L_tr = t_tr − t_final`。標本ごとの線は、Rust の標本では、その標本を閉じた `final` を区切った規則(`hayamimi` の stdout の `asr.endpoint.decided` の `rule`)で選ぶ。`early` なら 120 ms(`EARLY_MIN_SILENCE_MS`)、それ以外(`silence`、`max_speech`、`flush`)なら `min_silence`。Python の標本は `min_silence`。`e2e_columns.jsonl` を Rust として読む行には、隣に `e2e_columns.events.jsonl`(`final` 4 件、`id` は `mic-1`〜`mic-4`、`t_us` は各標本の `t_final_us` と同じ 2300000、2300000、2000000、1800000。1 件目と 2 件目は同じ `t_us` で規則も同じ `silence` なので、落ちる形(規則が違う 2 つ)に当たらない)と `e2e_columns.hayamimi.stdout.log`(`asr.endpoint.decided` 4 行、`id` は 1〜4、`rule` は全部 `silence`)を置く。Rust の run の stdout が無い run は落ちる。ただし 3.19 の「門の前の記録」の一覧に当たる `e2e1` と `e2e2` は、全標本を `min_silence` で数え、出力にその旨を書く(016〜019)。
**F88**(線の確かめ。式で作る)：Rust の run 1 本、標本 1 件。`t_end_us` 1000000、`t_final_us` 1300000、`final_latency_ms` 100.0(`L_asr` 300 ms、`L_asr − L_dec` = 200 ms)。events に `final` が 1 件(`id` は `mic-1`、`t_us` は標本の `t_final_us` と同じ 1300000。結ぶ鍵は 3.1 のとおり、`t_us` の一致で `final` を引き、`id` の数字 1 を `decided` の `id` と結ぶ)、stdout に `asr.endpoint.decided` が 1 行(`id` は 1)。その行の `rule` を `early`(`silence_ms` 120)にした物を **F88e**、`silence`(`silence_ms` 300)にした物を **F88s** とする。
command（受入 test の外）：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test e2e1 --test e2e2`。受入 test は `cargo test --release --test e2e1 -- --ignored`（と `e2e2`）で、記録が在る時だけ打つ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-E2E-1-001 | 実走の記録：Rust の標本 `e2e1.jsonl`(`e2e1.hayamimi.stdout.log` を隣に置く)と、Python の run の `py<N>.jsonl`（束ねる） | 受入 | 訳の取れた割合 ≥ 20 / 23、`L` の p50 と p95 が Python 以下 | 有効な標本（3.1 の標本ごとの線。Rust は `rule` で選び、Python は `min_silence`）のうち訳の取れた割合が 20 / 23 以上で、Rust の `L` の p50 と p95 がともに Python を束ねた値以下。`L`、`L_asr`、`L_tr` の p50 と p95 を出力する（specified、D の E2E-1 の期待値） | 陽性：D の「`<base>` の Python の run から同じ列が出る」は 010 と 013。陰性：訳が 0 件の記録で落ちる（005） | `e2e1_mic_process_ja_to_en_beats_python` |
| T-E2E-1-002 | `e2e_columns.jsonl` を Rust として数える(隣の events と stdout は全行 `silence`。標本ごとの線は 4 件とも 300 ms) | 無効な標本（1 件）と訳の無い標本（1 件）が混ざる | 有効 3、訳あり 2 | 有効 3（2 件目は `1300 − 1000.5 = 299.5 ms` で、その標本の線 300 ms 未満）、訳あり 2（1 件目と 3 件目）。`L` は 1500 と 2000 で n=2、p50 1500、p95 2000。`L_asr` は 800、1000、1300 で n=3、p50 1000、p95 1300。`L_tr` は 200 と 1000 で n=2、p50 200、p95 1000（derived、3.1 の式と nearest-rank の p95 = ceil(0.95 × n) 番目を fixture の数に手で当てた） | 境界（299.5 ms） | `the_columns_count_only_valid_samples` |
| T-E2E-1-003 | 標本が 0 件の run | 空 | 列が無い | 有効 0、訳あり 0、`L` と `L_asr` が無し（specified、3.1 の「標本が無い run は値を持たない」。0 件を速さと読まない） | 境界、陰性 | `a_run_with_no_sample_has_no_column` |
| T-E2E-1-004 | Python は `e2e_columns.jsonl`。Rust は同じ標本の全件に、`t_final_us` の 100000 µs 後の訳を付けた run | Rust が速く、全件に訳が在る | 通る | 判定の失敗が 0 件（derived、有効 3 の全件に訳が在り、`L` は 1400、1100、900 ms で p50 1100 ≤ Python の 1500、p95 1400 ≤ 2000） | 陽性 | `a_run_faster_than_python_and_fully_translated_passes` |
| T-E2E-1-005 | 004 と同じ組で、Rust の訳を `t_final_us` の 1000000 µs 後に付けた run | Rust が遅い | 落ちる | 失敗が 1 件で、p50 は Rust 2000 ms > Python 1500 ms、p95 は Rust 2300 ms > Python 2000 ms の両方を言う（derived、`L` が 2300、2000、1800 ms になる） | 陰性 | `a_run_slower_than_python_fails_on_both_percentiles` |
| T-E2E-1-006 | Python と Rust が同じ `e2e_columns.jsonl` | 百分位が等しい | 等しいは遅くない | 失敗が 1 件で、訳の割合（有効 3 のうち 2）だけを言う。p50 と p95 は失敗に入らない（specified、RP-3 の「p50 と p95 以下」の以下） | 境界 | `equal_percentiles_are_not_slower` |
| T-E2E-1-007 | Rust が `e2e_no_translation.jsonl`（3 件とも訳が無い）。Python は `e2e_columns.jsonl` | 訳が 0 件 | 落ちる | 失敗に `0 of 3 valid samples have a translation` と `rust: no valid sample has a translation` が入る（specified、D の E2E-1 の陰性対照「訳が 0 件で test が落ちる」。llama-server を止めた run の記録の形） | 陰性 | `a_run_with_no_translation_fails` |
| T-E2E-1-008 | Rust の標本が空 | 空の run | 通らない | 失敗に `no valid sample` が入る（specified、空を合格と読まない） | 陰性 | `an_empty_run_is_not_a_pass` |
| T-E2E-1-009 | `docs/results/r8h_verify/` の Rust の `rs1.jsonl` と、Python の `py1.jsonl` と `py2.jsonl`（束ねる） | 実物の記録 | 数が jq と一致 | Rust は有効 40、訳あり 34。Python は有効 80、訳あり 76。出力に `e2e1 rust: 34 of 40 valid samples translated` と `e2e1 python: 76 of 80 valid samples translated` が入る（derived、同じ file を `jq` で数えた値。数え方は test の注釈に在る。`rs1` は門の前の記録の一覧に在り、早い規則を持たない build の run なので、全標本を `min_silence` で数える(3.1 の例外)） | 陽性 | `a_recorded_run_gives_its_columns` |
| T-E2E-1-010 | `r8h_verify` の dir で `rs1` の記録の一覧 | 記録の置き場 | 名前の順 | Rust は `rs1.jsonl`、Python は `py1.jsonl`、`py2.jsonl` の順。`py1.events.jsonl` と `py1.summary.json` は run に数えない（specified、D の E2E-1 の「同じ列を `<base>` の Python の run から出せる」） | 陽性 | `a_run_directory_lists_its_samples_and_the_python_runs_in_name_order` |
| T-E2E-1-011 | 記録の無い run の名前 | 記録が無い | 言う | 失敗が「E2E の席が写す」旨を言う（derived、実走の記録は E2E の席が置く） | 陰性 | `a_run_directory_without_the_samples_says_what_to_copy` |
| T-E2E-1-012 | run の dir の名前 `e2e1` | 置き場の固定 | 末尾が一致 | `docs/results/e2e/e2e1` で終わる（specified、D の E2E の表の記録の置き場） | 境界 | `the_fixed_directory_of_a_run_is_under_docs_results_e2e` |
| T-E2E-1-013 | Python の run が 0 本の記録 | Python が無い | 断る | 失敗が `py<N>.jsonl` の名前を言う（specified、`L` は Python の run と比べて判定する） | 陰性 | `e2e1_without_a_python_run_is_refused` |
| T-E2E-1-014 | 標本の file `/r/obs2.jsonl` | events file の名前 | 隣に在る | `/r/obs2.events.jsonl`（derived、E2E-2 が events を読む規則と同じ） | 境界 | `the_events_file_stands_beside_the_samples_file` |
| T-E2E-1-015 | 有効 80 のうち訳あり 70 の run と 69 の run（Python は別に固定） | 割合の境界 20 / 23 | 70 は通り 69 は落ちる | 70/80 = 0.875 は 20/23 = 0.8696 以上で割合の失敗が 0 件。69/80 = 0.8625 は未満で割合の失敗が 1 件（derived、20/23 の除算。T-RP-43-009 の 70/80 の判定がここに在る）。`L` は Python 以下に揃えて割合だけを動かす | 境界、陽性、陰性 | `the_translated_share_passes_at_70_of_80_and_fails_at_69` |
| T-E2E-1-016 | F88e を Rust の run として数える | 規則が `early` で `d` が 200 ms | 線 120 ms | 有効 1(specified、3.1 の標本ごとの線。`early` は `EARLY_MIN_SILENCE_MS` の 120 ms。200 ≥ 120) | 境界、陽性 | `an_early_sample_of_200_ms_is_valid` |
| T-E2E-1-017 | F88s を Rust の run として数える | 規則が `silence` で `d` が 200 ms | 線 `min_silence`(300 ms) | 有効 0、終端不正 1(specified、同上。200 < 300。全標本を 120 ms で数える build は有効 1 にして落ちる。016 の対) | 境界、陰性 | `a_silence_sample_of_200_ms_is_not_valid` |
| T-E2E-1-018 | 2 入力。(a) 一覧に無い Rust の run(名前 `rs9`)で、`rs9.hayamimi.stdout.log` が無い。(b) 一時 dir に写した `docs/results/e2e/e2e1/e2e1.jsonl` の末尾に 1 byte(改行)を足した物(sha256 が一覧と違う)で、stdout が無い | 一覧に無い Rust の run が stdout を持たない | stdout が在る | どちらも `Err` で run の名前と `hayamimi.stdout.log` が無いことを名指す(specified、3.1 の終端不正の例外「その run は落とす」と 3.19 の「門の前の記録」。名前が同じでも sha256 が違えば免除されない。016 の F88e が stdout つきで通ることが陽性対照) | 境界、陰性 | `a_rust_run_off_the_list_without_a_stdout_is_rejected` |
| T-E2E-1-019 | 一時 dir を repo の root に見立て、`docs/results/e2e/e2e1/e2e1.jsonl` を実物から写す。stdout は無い | 一覧に当たる `e2e1`(早い規則を持たない build の run) | 全標本を `min_silence` で数える | `Ok` の側で、全標本を 300 ms の線で数える。有効 100、訳あり 93。出力に、stdout が無く全標本を `min_silence` で数えたことが在る(specified、3.1 の例外。pseudo：`jq -s` で `t_final_us` が在り `(t_final_us − t_end_us)/1000 − final_latency_ms ≥ 300` の標本が 100、うち `t_tr_us` が在る物が 93。2026-10-21 に打った、枠は 115) | 陽性 | `the_listed_e2e1_run_is_counted_at_min_silence_without_a_stdout` |
| T-E2E-1-020 | 一時 dir を repo の root に見立て、`docs/results/r8h_verify/rs1.jsonl`(sha256 は `b3097191…`)を実物から写し、末尾に 1 byte(改行)を足す。stdout は無い | 一覧の `rs1` と名前が同じで sha256 が違う | 一覧に当たらない | `Err` で `rs1` と `hayamimi.stdout.log` が無いことを名指す(specified、3.19 の「門の前の記録」の 7。018 の (b) と同じ形。中身を差し替えた run を一覧の免除で通さない。足す前の写し(sha256 が一覧と同じ)は 009 が `Ok` で通ることが陽性対照) | 境界、陰性 | `a_listed_name_with_another_sha256_is_not_exempt_in_e2e1` |
| T-E2E-1-021 | F88 を Rust の run とし、events の `final` を 2 つにする(`id` は `mic-1` と `mic-2`、`t_us` は両方 1300000)。stdout の `decided` は `id` 1 と `id` 2。2 入力。(a) `id` 1 が `early`、`id` 2 が `silence`。(b) 2 つとも `early` | 同じ `t_us` に `final` が 2 つ。規則が違う側と同じ側 | 標本を閉じた規則が 1 つに決まる | (a) は `Err` で run の名前と `mic-1`、`mic-2` を名指す。(b) は `Ok` で有効 1(`d` 200 ms、線 120 ms)(specified、3.1 の終端不正の例外。規則が違う 2 つは落とし、同じなら線は 1 つに決まる。(b) が (a) の陽性対照) | 境界、陰性 | `two_finals_with_different_rules_at_one_t_us_are_rejected` |
| T-E2E-1-022 | F88 の stdout から `id` 1 の `decided` の行を除く(別の `id` 9 の行だけ残す) | `final` に結ぶ `decided` が無い | すべての `final` が `decided` と結べる | `Err` で run の名前と `mic-1` を名指す(specified、3.1 の「`decided` の行と結べない `final`」。016 の F88e が通ることが陽性対照) | 境界、陰性 | `a_final_without_a_decided_row_is_rejected` |
| T-E2E-1-023 | F88e の stdout に、`id` 1 の `decided` の行をもう 1 行足す(規則は同じ `early`) | 同じ `id` の `decided` が 2 行 | `id` ごとに 1 行 | `Err` で run の名前と `id` 1 を名指す。規則が同じでも落ちる(specified、3.1 の「同じ `id` の `decided` が 2 行在る run」。016 が 1 行で通ることが陽性対照) | 境界、陰性 | `two_decided_rows_with_one_id_are_rejected_even_with_the_same_rule` |
| T-E2E-2-001 | 実走の記録：Rust の標本 `e2e2.jsonl` と events file `e2e2.events.jsonl`(`e2e2.hayamimi.stdout.log` を隣に置く) | 受入 | `lang` が en の `final` と ja の訳、割合 ≥ 20 / 23、`L` の p95 ≤ 2000 ms | events に `lang` が `en` の `final` と、`lang` が `ja` の見える訳が在り、有効な標本(3.1 の標本ごとの線。Rust は `rule` で選ぶ)のうち訳の取れた割合が 20 / 23 以上で、`L` の p95 が 2000 ms 以下。p50 は出力するが判定しない（specified、D の E2E-2 の期待値） | 陽性：`/ingest` の run（008）。陰性：訳が 0 件の記録で落ちる（009） | `e2e2_loopback_process_en_to_ja_within_budget` |
| T-E2E-2-002 | 予算 `E2E2_P95_BUDGET_MS` と、`L` の p95 が 1999、2000、2001 ms の列 | 予算の境界 | 2000 は通り 2001 は落ちる | 予算は 2000。1999 と 2000 は失敗なし。2001 は `L p95 2001 ms > 2000 ms` を言う（specified、RP-3 の en→ja の「p95 が 2000 ms 以下」） | 境界、陰性 | `the_budget_is_2000_ms_and_the_boundary_passes` |
| T-E2E-2-003 | `L` が無い列（標本が無い） | `L` 無し | 通らない | 予算の失敗が在る（specified、空を予算の内と読まない） | 陰性 | `a_run_with_no_l_is_over_no_budget_and_still_fails` |
| T-E2E-2-004 | 9 行の events：en の `final` 2 件、ja の `final` 1 件、ja の見える訳 2 件（`status` が `ok`）、ほかに `status` が無い訳、en の訳、`held` の訳、空の訳、`of` が `partial` の訳 | 数える対象の仕分け | en の `final` 2、ja の見える訳 2 | en の `final` が 2、ja の見える訳が 2。ja の `final`、en の訳、`status` が `ok` でない訳、空の訳、`partial` の訳は数えない（specified、D の E2E-2 の「`lang` が `en` の `final` と `lang` が `ja` の訳」と、字幕に見える訳だけを数える H の規則） | 境界、陰性 | `the_events_are_counted_by_language_and_by_what_the_subtitle_shows` |
| T-E2E-2-005 | events の行が `not json`、`{"t_us":1}`、空の文字列 | 不正な行 | 断る | 前の 2 つは失敗、空は (0, 0)（derived） | 陰性、境界 | `an_events_line_that_is_not_an_event_is_refused` |
| T-E2E-2-006 | 全件に訳が在り速い run と、en の `final` と ja の訳が 1 件ずつの events | 通る | 通る | 判定の失敗が 0 件（derived） | 陽性 | `a_fast_translated_run_with_both_languages_passes` |
| T-E2E-2-007 | `L` が 2300、2000、1800 ms の run | p95 だけが予算を超える | 1 件だけ落ちる | p95 は最大の 2300 ms。失敗が 1 件で `L p95 2300 ms > 2000 ms`（derived、nearest-rank の p95 = ceil(0.95 × 3) = 3 番目） | 陰性 | `a_run_whose_p95_is_over_2000_ms_fails_on_that_alone` |
| T-E2E-2-008 | `r8h_verify` の `/ingest` の run `e2e2c.jsonl` と `e2e2c.events.jsonl` | 実物の記録 | 数が jq と一致 | 有効 30、訳あり 25、en の `final` 38、ja の見える訳 54。出力に `e2e2 rust: 25 of 30 valid samples translated` と `38 final in en, 54 shown translation in ja` が入る（derived、同じ file を `jq` で数えた値。D の E2E-2 の陽性対照「`/ingest` の run でも同じ列が出る」。`e2e2c` は門の前の記録の一覧に在り、早い規則を持たない build の run なので、全標本を `min_silence` で数える(3.1 の例外)） | 陽性 | `a_recorded_run_gives_its_columns_and_event_counts` |
| T-E2E-2-009 | 訳が 0 件の記録（`e2e_no_translation.jsonl`） | 訳が 0 件 | 落ちる | 失敗に `0 of 3 valid samples have a translation`、`rust: no valid sample has a translation`、`no shown translation in ja` が入り、`no final in en` は入らない（specified、D の E2E-2 の期待値。en の `final` は在り ja の訳だけが無い） | 陰性 | `a_run_with_no_translation_fails` |
| T-E2E-2-010 | 標本も events も空の run | 空 | 全部の数で落ちる | 失敗に `no valid sample`、`hold no final in en`、`hold no shown translation in ja` が入る（specified、空を合格と読まない） | 陰性、境界 | `an_empty_run_with_no_events_fails_on_every_count` |
| T-E2E-2-011 | 割合の境界：有効 23 のうち訳あり 20 の run と 19 の run | 20 / 23 の境界 | 20 は通り 19 は落ちる | 20/23 は割合の失敗が 0 件、19/23 は 1 件。events と `L` は通る値に揃える（derived、20/23 と同じ除算） | 境界、陰性 | `the_translated_share_boundary_is_20_of_23` |
| T-E2E-2-012 | 一時 dir を repo の root に見立て、`docs/results/e2e/e2e2/e2e2.jsonl` と `e2e2.events.jsonl` を実物から写す。stdout は無い | 一覧に当たる `e2e2` | 全標本を `min_silence` で数える | 全標本を 300 ms の線で数え、有効 75、訳あり 70。出力に、stdout が無く全標本を `min_silence` で数えたことが在る(specified、3.1 の例外。pseudo：`jq -s` で 75 件とも `t_final_us` が在り線 300 ms 以上、`t_tr_us` が在る物が 70。2026-10-21 に打った) | 陽性 | `the_listed_e2e2_run_is_counted_at_min_silence_without_a_stdout` |
| T-E2E-2-015 | 一時 dir を repo の root に見立て、`docs/results/r8h_verify/e2e2c.jsonl`(sha256 は `804e398e…`)と `e2e2c.events.jsonl` を実物から写し、`e2e2c.jsonl` の末尾に 1 byte(改行)を足す。stdout は無い | 一覧の `e2e2c` と名前が同じで sha256 が違う | 一覧に当たらない | `Err` で `e2e2c` と `hayamimi.stdout.log` が無いことを名指す(specified、同上。T-E2E-1-020 の e2e2 側。足す前の写しが 008 で `Ok` になることが陽性対照) | 境界、陰性 | `a_listed_name_with_another_sha256_is_not_exempt_in_e2e2` |
| T-E2E-2-016 | T-E2E-1-021 の (a)、022、023 の 3 入力を e2e2 の test に Rust の run として渡す | e2e2 の test が 3 つの落とす形を持つ | 3 つとも落とす | 3 入力とも `Err`。021 の (a) は run の名前と `mic-1`、`mic-2`、022 は run の名前と `mic-1`、023 は run の名前と `id` 1 を名指す(specified、3.1。E2E-1 の 021 から 023 と同じ線を e2e2 の test が持つ。021 の (b) の `Ok` も同じに通る) | 境界、陰性 | `the_three_rejected_rule_shapes_are_rejected_in_e2e2_as_in_e2e1` |
| T-E2E-2-013 | 2 入力。(a) F88e を e2e2 の test に Rust の run として渡す。(b) F88s を渡す。(c) 一覧に無い Rust の run で stdout が無い | 標本ごとの線と stdout の欠け(E2E-1 の 016、017、018 と同じ線を e2e2 の test が持つ) | 線 120 ms と 300 ms | (a) は有効 1、(b) は有効 0 で終端不正 1、(c) は `Err` で run の名前と `hayamimi.stdout.log` が無いことを名指す(specified、同上) | 境界、陰性 | `the_per_sample_line_and_a_missing_stdout_are_judged_in_e2e2_as_in_e2e1` |
| T-E2E-2-014 | L9 の形の記録：`docs/results/r8x/e1pre.e2e2.jsonl`、`e1pre.e2e2.events.jsonl`、`e1pre.e2e2.hayamimi.stdout.log`(L9 の en の run。main に在る) | 陽性対照。標本 74 件のうち early で閉じた標本が 57 件(うち 39 件は gap が 300 ms 未満)、silence で閉じた標本が 17 件 | 標本ごとの線と 300 ms の線の両方を記録 | 標本ごとの線では n 74、p95 1318 ms。300 ms の線では n 35、p95 2879 ms。出力に 2 つの n と p95 を並べる(specified、H88。標本ごとの線で数えない build は n 35 で落ちる。数え方の違いが p95 を 1561 ms 動かすことを記録に残す) | 陽性、境界 | `the_l9_record_counts_74_samples_by_the_per_sample_line_and_35_by_300_ms` |

実装済みの行は、E2E-1 の 001 から 023 と E2E-2 の 001 から 016 である。E2E-1 の行は `native/hayamimi-eval/tests/e2e1.rs`、E2E-2 の行は `native/hayamimi-eval/tests/e2e2.rs` に在る。標本ごとの線と stdout の結びは `tests/support/e2e.rs` と `tests/support/rp32_rules.rs` が持つ。
陰性の build と落ちる行（表から読んだ予測、NOT VERIFIED）：割合を数えない build は 007、009、015、E2E-2-011 が落ちる。`L` の p50 だけを判定する build は 005 が落ちる。0 件を合格と読む build は 003、008、E2E-2-003、010 が落ちる。ja の `final` を数える build は E2E-2-004 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- D の E2E-1 の陰性対照のうち「`hayamimi` と中継は止まらず `translate.fallback.held` を出す」と、E2E-2 の「出力の装置を止めた run では止まらず `capture.fallback.none` が 1 件」は、記録を読む test の外の確かめで、この節の行にしていない。機械で確かめる在り処は設計に無い（持ち主は E2E の席）。
- 受入 test は記録を読み、何も起こさない。D の E2E の表の command（llama-server、中継、process A と B を起こす）を、誰がどの手番で打って記録を置くか（`docs/results/e2e/` への写し）は、設計の 8 の R8 の行に在るが、この節は触れない。
- 009 の `rs1` と E2E-2-008 の `e2e2c`(`docs/results/r8h_verify/`)は、門の前の記録の一覧に在る(3.19 の「門の前の記録」の 7、3.1 の終端不正の例外)。標本と規則を結ぶ鍵も 3.1 に在る。
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
| T-RP-66-056 | **文の途中の切断の率**：ja の R8v を合わせた枠の全標本 230（F66J の 2 本。有効 200 + final なし 30）に、文の途中の割れ 3 件と本物 2 件と尾の雑音 1 件。もう 1 つの run は文の途中 0 件 | 率の式 | 3 ÷ 230 と 0 | 率 = 3 ÷ 230（約 0.01304）。分母は有効な標本の 200 ではなく枠の全標本の 230 で、3 ÷ 200 = 0.015 と読む実装は落ちる。0 件の run は 0（derived、D 3.19 の RP-86 の (2) 「率の分母は枠の全標本」と D 6 の RP-66 の「文の途中と仕分けた割れの数 ÷ 標本の数」。本物と尾の雑音は分子に入らない）。この行は率の値だけを確かめる。判定は 057 | 陽性、境界 | `the_mid_sentence_cut_rate_counts_only_mid_sentence_splits_over_all_framed_samples` |
| T-RP-66-057 | 上限 U を置いた後の 056 の形。文の途中の割れが ⌊U × 230⌋ 件の run と、1 件多い run（枠の全標本 230） | 上限の両側 | 前者は `Ok`、後者は `Err` | 前者は `Ok`（率 ≤ U の等号側）、後者は `Err` で文の途中の切断の率を名指し。U の値は評議が手番 1b の後に固定する。値が固定されるまで、この行の数は書かない（specified、D 6 の RP-66 の「上限以下」） | 境界、陰性 | `the_mid_sentence_cut_rate_limit_is_inclusive` |
| T-RP-66-058 | 完結の判定を常に真にした build の run を模した入力。文の途中の割れが ⌊U × 230⌋ + 1 件以上（枠の全標本 230） | 毎回の候補で確定する | `Err` | `Err` で文の途中の切断の率を名指し。割れの数（`finals` が 2 以上の標本の数）は記録だけで、これでは落ちない（specified、D 6 の RP-66 の陰性欄）。U は評議が手番 1b の後に固定する | 陰性 | `an_always_complete_build_fails_on_the_mid_sentence_cut_rate` |
| T-RP-66-059 | 割れた標本の訳の chrF：(1) 前の文の訳と後ろの文の訳をつないだ物が、A の 1 つの final の訳と同じ文字列。(2) 共通する字が 1 つも無い文字列 | 記録だけの欄 | 100 と 0。判定に入らない | (1) の chrF は 100、(2) は 0 を出力に書く。どちらの値でも他の判定は変わらず `Ok` のまま（derived、chrF の定義。D の数え方の 8 は「合否に入れない」） | 陽性、境界 | `the_chrf_of_split_translations_is_recorded_and_not_judged` |
| T-RP-66-060 | **最後の final の遅れ**：10 発話。発話 k の `end_sample` は 10 秒、k = 1〜10 の最後の final の検出が 100k ms 後。ただし発話 3 は final が 2 つで、最初が 300 ms 後、最後（B だけの割れの後ろの文）が 800 ms 後 | 割れた発話の遅れ | p50 600、p95 1000 | 遅れの列は 100、200、800、400、500、600、700、800、900、1000。小さい順に 100、200、400、500、600、700、800、800、900、1000。p50 は (50 × 10 + 99) / 100 = 5 番目で 600 ms、p95 は (95 × 10 + 99) / 100 = 10 番目で 1000 ms（derived、D の数え方の 10。最初の final の 300 ms で取ると p50 は 500 ms になる） | 陽性、陰性 | `delay_is_taken_at_the_last_final_of_the_utterance` |
| T-RP-66-061 | 060 の発話 3 を final 1 つ（300 ms 後）にした run | 割れの無い発話 | p50 500、p95 1000 | 遅れの列は 100〜1000 の 10 個。p50 は 5 番目の 500 ms、p95 は 10 番目の 1000 ms（derived）。060 との差が最後の final の効き目 | 境界 | `an_unsplit_utterance_is_delayed_at_its_only_final` |
| T-RP-66-062 | 060 の発話 10 を、A の segment が 1 つも触れない発話にする | 母数から外す発話 | n 9、除外 1 | n は 9。遅れの列は 100、200、800、400、500、600、700、800、900 で、小さい順の 5 番目は 600 ms（p50）、9 番目は 900 ms（p95。(95 × 9 + 99) / 100 = 9）。除外した発話の数 1 を別の欄に書く（derived、D の数え方の 10。除外を遅れ 0 と読まない） | 境界、陰性 | `an_utterance_no_segment_touched_is_left_out_and_counted_apart` |
| T-RP-66-063 | 060 の発話の列と、A だけの検出の時刻の列（発話 k が 100k + 50 ms 後。割れ無し）を同じ関数に渡す | 比べる相手は同じ数え方の A だけの値 | 2 つの列 | R8v の列は p50 600、p95 1000。A だけの列は 150、250、…、1050 の 10 個で p50 5 番目の 550、p95 10 番目の 1050 を並べて出す（derived、D の数え方の 10。同じ nearest-rank） | 陽性 | `the_a_only_delay_is_counted_the_same_way_and_written_beside` |
| T-RP-66-064 | 001 の ja の 4 run（Rust の run。それぞれ T-RP-83-001 の形の `<run>.machine.jsonl` を持つ）で、4 番目の run（R8v の 2 本目）の `<run>.machine.jsonl` の行 1 の `load15` を `"2.50"` にする | E2E の run の門を呼ぶ確かめ（RP-83 の線の値） | `load15` < 2.50 | `Err` で 4 番目の run の名前、`load15`、`2.50` を名指す。ほかの 3 run の名前は名指さない（specified、3.19 の「E2E の run の門」の 5「呼ぶことの確かめ」。判定の関数が RP-83 の support を呼び忘れた test は、ここで `Ok` を返して落ちる。4 run 全部が内側の 001 が陽性対照） | 陰性 | `a_run_with_a_first_row_load15_of_2_50_is_rejected_by_the_gate` |
| T-RP-66-029 | 001 の ja で、4 番目の run の前の load を `1.99 0.50 0.50 1/300 1000` にする | load の上側の境界の手前 | `Ok` | `Ok`（specified、6 の RP-66 の「RP-32 と同じ」と RP-32 の「2 未満」。199 < 200） | 境界、陽性 | `a_load_of_1_99_before_a_run_passes` |
| T-RP-66-030 | 029 の load を `2.00 0.50 0.50 1/300 1000` にする | 境界の値 | `Err` | `Err` で 4 番目の run を名指し（specified、RP-32。200 は 2 未満でない） | 境界、陰性 | `a_load_of_2_00_before_a_run_is_rejected` |
| T-RP-66-031 | 001 の ja で、先頭の 1 対だけ（main、R8v の 2 本） | 対が 1 つ | `Err` | `Err` で対の数が 2 でないことを名指し（specified、6 の RP-66 の「言語ごとに 2 対」） | 境界、陰性 | `one_pair_per_language_is_rejected` |
| T-RP-66-032 | ja。合わせた main の `d` の p50 が 804、805、925、926 ms の 4 入力 | 記録の 865 ms の ±60 ms の帯の両端 | 805 と 925 は `Ok`、804 と 926 は `Err` | 805（865 − 60）と 925（865 + 60）は帯の内で `Ok`、804 と 926 は `Err` で main の `d` の p50 を名指し（specified、D 3.17 の数え方の 4 の「差の絶対値 ≤ 60」、6 の RP-66 の陽性欄）。落ちた入力は assert の message に出す | 境界 | `main_d_p50_outside_the_recorded_band_for_ja_is_rejected` |
| T-RP-66-033 | en。合わせた main の `d` の p50 が 396、397、517、518 ms の 4 入力 | 記録の 457 ms の ±60 ms の帯の両端 | 397 と 517 は `Ok`、396 と 518 は `Err` | 397（457 − 60）と 517（457 + 60）は帯の内で `Ok`、396 と 518 は `Err` で main の `d` の p50 を名指し（specified、同上） | 境界 | `main_d_p50_outside_the_recorded_band_for_en_is_rejected` |
| T-RP-66-034 | 実物の記録：`docs/results/e2e/e2e1/e2e1.jsonl` と `docs/results/e2e/e2e2/e2e2.jsonl` を main の run として渡す | 実物の main の `d` | `Ok` | 有効な標本（ja n 100、en n 75）の nearest-rank で、ja の main の `d` の p50 は 865.5 ms、en は 456.6 ms で、どちらも帯の内側（805〜925、397〜517）で、D の ja 865 ms、en 457 ms と丸めて合う（pseudo、jq。`select(.t_final_us != null)` で `d ≥ 120`。2026-10-15 に打った）。D 3.17 の冒頭の表の 881 と 443 は訳の取れた標本の値で、この行には使わない（D 3.17 の数え方の 1） | 陽性 | `the_recorded_main_run_is_inside_the_band` |
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

### 4.65 RP-67（主指標 `L` の p95 を 1600 ms 以下にする）

**実装済み。** 19 行の関数は、どれも `native/hayamimi-eval/tests/rp67.rs` に 1 本ずつ在る（各行の「関数の末尾」の名前で `grep -c "^fn <名前>("` が 1）。判定は `support/machine.rs` の `check_runs` を呼び、p95 は `percentile::nearest_rank` をマイクロ秒のまま 1600 ms の線と比べる（丸めた ms では比べない）。
合否の線は 1600 ms で、1200 ms と 1000 ms は目標である（D 3.19、H132）。test は 1200 ms を超える標本の数と 1200 ms での判定も、目標の側の記録として並べて書く。
test：`native/hayamimi-eval/tests/rp67.rs`。受入 test の名前は `rp67_speech_end_to_translation_p95_is_within_1600_ms`。実走の行（T-RP-67-015）だけが `#[ignore]` を持ち、`docs/results/rp67/` の `e2e1.jsonl` と `e2e2.jsonl` を読む。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp67`。実走の行はこれに `-- --ignored` を足す。
関数の名前と引数の形は実装の席が決める。行は「言語ごとの run の標本の列を渡す」形で書く。

**数え方**（3.1 と D 3.17 の数え方の 3 のとおり）：`L = t_tr − t_end`。n は有効（`final` が在り `d ≥ min_silence`、RP-67 も `min_silence` は 0.12 秒。評議が GO）で訳の取れた標本の数。test は 0.30 秒でも数えて並べて記録する。p95 は nearest-rank で順位は (95 × n + 99) / 100 の整数除算。したがって 1600 ms を超えてよい標本の数は `n − 順位` である。

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

**fixture F67**：n 標本、全部 `d` 500 ms、`L_dec` 150 ms、`L_tr` 250 ms（`L` は 900 ms）。「k 件を超える」は、`L` の大きい方から k 件の `L_tr` を足して `L` を 1601 ms にする。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-67-001 | F67、n = 93（ja）と n = 70（en） | 全標本が 900 ms | `Ok` | 両方 `Ok`（specified、6 の RP-67 の「全標本の `L` を 900 ms に置いた fixture で通る」）。出力に `d` 500 ms、復号 150 ms、訳 250 ms、`L` 900 ms の p50 と p95 が並ぶ（specified、「同じ結果に並べて書く」） | 陽性 | `all_samples_at_900_ms_pass_and_the_columns_are_written` |
| T-RP-67-002 | F67、n = 93 で全標本の `L` を 1600 ms にする | 限度の値 | `Ok` | `Ok`（specified、6 の RP-67 の「1600 ms 以下」。等号側）。1200 ms の目標の側の記録は 93 件が超えると出す | 境界、陽性 | `l_of_exactly_1600_ms_passes` |
| T-RP-67-003 | F67、n = 93 で 4 件を 1601 ms 以上にする | 超えてよい数ちょうど（ja） | `Ok` | `Ok`。89 番目は 900 ms（derived、(95 × 93 + 99) / 100 = 89。n − 89 = 4 件まで超えてよい） | 境界、陽性 | `four_over_in_93_samples_still_pass` |
| T-RP-67-004 | F67、n = 93 で 5 件を 1601 ms 以上にする | 超えてよい数より 1 件多い（ja） | `Err` | `Err` で `L` の p95 を名指し。89 番目が 1601 ms 以上（derived、同上。D の「許す数より 1 件多く入れた fixture で落ちる」） | 境界、陰性 | `five_over_in_93_samples_are_rejected` |
| T-RP-67-005 | F67、n = 70 で 3 件を 1601 ms 以上にする | 超えてよい数ちょうど（en） | `Ok` | `Ok`。67 番目は 900 ms（derived、(95 × 70 + 99) / 100 = 67。n − 67 = 3） | 境界、陽性 | `three_over_in_70_samples_still_pass` |
| T-RP-67-006 | F67、n = 70 で 4 件を 1601 ms 以上にする | 超えてよい数より 1 件多い（en） | `Err` | `Err` で `L` の p95 を名指し。67 番目が 1601 ms 以上（derived、同上） | 境界、陰性 | `four_over_in_70_samples_are_rejected` |
| T-RP-67-007 | F67、n が 92、94、95、96（ja の側）と 69、71（en の側）の各 n で、超える件数が表の数ちょうどの入力と 1 件多い入力 | n が動くと順位が動く | 表の数は `Ok`、1 件多いと `Err` | 表のとおり。n = 92 は 4 件で `Ok`、5 件で `Err`。n = 69 と 71 は 3 件で `Ok`、4 件で `Err`（derived、順位の式を各 n に当てた。順位を 93 と 70 の値に固定した build はここで落ちる） | 境界 | `the_allowed_count_follows_n` |
| T-RP-67-008 | main の記録の形：n = 70 で、上位 3 件が 4681、1658、1603 ms、次の 9 件が 1201〜1600 ms、残りが 900 ms | en の記録の形 | `Ok` | `Ok`。1600 ms を超えるのは上位 3 件で、許す数 3 ちょうど。67 番目は 1600 ms 以下（derived、(95 × 70 + 99) / 100 = 67）。1200 ms の目標の側の記録は 12 件が超えると出す（12 件は記録の jq の数え直し） | 境界、陽性 | `a_run_shaped_like_the_recorded_en_run_passes_the_1600_ms_line` |
| T-RP-67-009 | 008 の 9 件のうち 1 件を 1601 ms にした n = 70 | 1600 ms を超える標本が 4 件 | `Err` | `Err` で `L` の p95 を名指し。67 番目が 1601 ms 以上（derived、同上。上位 3 件が枠を使い切り、4 件目で落ちる）。008 の対照 | 境界、陰性 | `a_fourth_sample_over_1600_ms_is_rejected_in_70_samples` |
| T-RP-67-010 | n = 100 の有効のうち、訳の取れない標本が 7（`t_tr_us` が null）、訳の取れた 93 のうち 5 件が 1601 ms 以上、88 件が 900 ms | 訳の無い標本を `L` が小さいと読む | `Err` | `Err`。n は 93、89 番目が 1601 ms 以上。訳の無い 7 標本を `L` 0 として 100 標本に混ぜると 95 番目が 900 ms になり通ってしまう（derived、(95 × 93 + 99) / 100 = 89。実物の記録は ja が有効 100 のうち訳ありが 93） | 陰性 | `samples_without_a_translation_are_not_counted_as_fast` |
| T-RP-67-011 | 訳の取れた有効な標本が 0 件の run | 空 | `Err` | `Err` で標本が無いことを名指し（specified、E2E-1 の T-E2E-1-008 と同じ。0 件を「1600 ms 以下」と読まない） | 陰性、境界 | `a_run_with_no_translated_sample_is_not_a_pass` |
| T-RP-67-012 | ja は全標本 1601 ms、en は全標本 900 ms | ja だけが落ちる | `Err` | `Err` で ja（E2E-1）だけを名指し。en は名指さない（specified、6 の RP-67 の「それぞれの `L` の p95」） | 陰性 | `a_failing_ja_run_is_named_alone` |
| T-RP-67-013 | ja は全標本 900 ms、en は全標本 1601 ms | en だけが落ちる | `Err` | `Err` で en（E2E-2）だけを名指し。ja は名指さない（specified、同上） | 陰性 | `a_failing_en_run_is_named_alone` |
| T-RP-67-014 | 今の main の記録：`docs/results/e2e/e2e1/e2e1.jsonl` と `docs/results/e2e/e2e2/e2e2.jsonl`（git の中に在る。D 3.17 の数え方の 6） | 実物の記録 | ja は `Err`、en は `Ok` | ja は n 93 の `L` の p95 が 1871.7 ms（89 番目）で `Err`。en は n 70 の p95 が 1303.2 ms（67 番目）で 1600 ms の内側の `Ok`、1200 ms の目標の側の記録は超えたと出す。`L` が 1200 ms 以下の標本は ja が 28、en が 58（pseudo、jq。`select(.t_final_us != null and .t_tr_us != null)` と `d ≥ 300`、nearest-rank。p95 と順位は 2026-10-15、1200 ms 以下の数は 2026-10-20 に打った。1000 ms 以下は ja が 6、en が 38 で、目標の側の記録として残す）。出力に `d` の p50 881.5、p95 1355.1 ms（ja）、443.6、671.9 ms（en）も並ぶ | 陽性、陰性 | `the_recorded_main_ja_run_is_over_1600_ms_and_the_en_run_is_inside` |
| T-RP-67-016 | **S2**：main の記録（014 の 2 本）。陰性対照として、ja の 1 標本の `d` を 250 ms にした main の run | main の run で 0.12 秒と 0.30 秒の有効の数 | 一致なら `Ok`、差が出たら `Err` | 記録は ja が 100 と 100、en が 75 と 75 で、0.12〜0.30 秒に入る標本は 0 件で `Ok`（pseudo、jq。`select(.t_final_us != null and d >= 120)` と `>= 300` を数えた。評議の打ち直しと同じ値。2026-10-15 に打った）。250 ms の標本を入れた run は 0.12 秒で 100、0.30 秒で 99 になり、`Err` で main の run の有効の数が合わないことを名指す（評議の S2。差が出たら落とす） | 陽性、陰性、境界 | `main_runs_count_the_same_valid_samples_at_120_and_300_ms_for_the_l_verdict` |
| T-RP-67-017 | **S3**：n 93 の run。88 標本は `d` 500 ms で `L` 900 ms。4 標本は `d` 500 ms で `L` 1601 ms。1 標本は `d` 250 ms で `L` 1900 ms（訳が取れている） | 0.12 秒でだけ有効な標本が 1 つ。数え方で判定が割れる | `Err` と並べた記録 | 0.12 秒の判定は n 93 で 5 件が 1600 ms 超、89 番目が 1601 ms で `Err`。0.30 秒で数えると n 92、4 件が超えて 88 番目が 900 ms になり通る。出力に 0.12 秒でだけ有効な標本の数 1 とその `L` 1900 ms を別に書き、`L` の p95 を 0.12 秒の値 1601 ms と 0.30 秒の値 900 ms の両方で並べる（derived、(95 × 93 + 99) / 100 = 89 と (95 × 92 + 99) / 100 = 88。評議の S3。0.30 秒で数える build はここで合格してしまうので落ちる） | 陰性、境界 | `a_slow_sample_valid_only_at_120_ms_counts_and_both_p95_values_are_written` |
| T-RP-67-018 | **S4**：n 93 の訳の取れた標本（全部 `L` 900 ms）に、訳の取れない有効な標本 7（`t_tr_us` が null）を足した run | 訳の取れない有効な標本 | `Ok` と数 | `Ok`。出力に訳の取れない有効な標本の数 7 が別の欄で出る。7 標本は n に入らない（010 と同じ）（derived、評議の S4。実物の記録は ja が 7、en が 5。jq の `select(.t_final_us != null and .t_tr_us == null)`） | 陽性 | `valid_samples_without_a_translation_are_counted_apart` |
| T-RP-67-019 | T-RP-67-001 の ja の run（Rust。n = 93）に T-RP-83-001 の形の `<run>.machine.jsonl` を付け、行 1 の `load15` を `"2.50"` にする。en の run は内側の形の file を持つ | E2E の run の門を呼ぶ確かめ（RP-83 の線の値） | `load15` < 2.50 | `Err` で ja の run の名前、`load15`、`2.50` を名指す。en の run の名前は名指さない。`L` の p95 が 900 ms で線の内側でも `Ok` を返さない（specified、3.19 の「E2E の run の門」の 5。判定の関数が support を呼び忘れた test は、ここで落ちる。ja も en も内側の形の 001 が陽性対照） | 陰性 | `a_rust_run_with_a_first_row_load15_of_2_50_is_rejected_by_the_gate` |
| T-RP-67-015 | 実走：`docs/results/rp67/` の `e2e1.jsonl`（23 発話 × 5 回）と `e2e2.jsonl`（15 発話 × 5 回） | 実走 | 実走 | 両言語で `L` の p95 ≤ 1600 ms で `Ok`、`d`、復号、訳の p50 と p95、1200 ms を超える標本の数（目標の側の記録）も出す（specified、6 の RP-67）。記録が置かれるまでは走らせない | 陽性 | `rp67_speech_end_to_translation_p95_is_within_1600_ms` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- RP-67 の有効の線は 0.12 秒で、評議が GO を出した（S2、S3、S4 は 016〜018）。D の 6 の RP-67 の行（`rust_port.ja.md:978`）にもこの線が書かれている。RP-67 の `L` は `t_tr − t_end` で取り、発話の終わりの遅れ（最後の final）の数え方は使わない。
- RP-67 の run が 1 回か複数回か（D は「5 回」の枠だけを書き、交互の対にしない）。この節は 1 つの run の列を渡す形にした。
- 015 の `docs/results/rp67/` の記録を、誰がどの手番で置くか。

**設計の席が決めた物**：RP-67 の `L` の起点は `t_end`（manifest の `end_sample`）のままで、動かさない（D 3.1 の「起点の確かさ」、D 3.17）。起点の確かさの組（確か、切れ、不確か）ごとの `L` の p95 は RP-67 の出力に並べて書き、合否が起点で割れないかは RP-73（4.71）が見る。

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

### 4.70 E の card の形、早い終わりの reason、初回の依頼

設計は D 3.6 の「E の依頼の形」と「初回の依頼」、D 3.17、D 3.13 の `asr.endpoint.held` の行である。
test の file と関数は repo に在る。command は `cd native && cargo test --release --jobs 2 -p hayamimi-relay -p hayamimi-asr -p hayamimi -p hayamimi-eval --lib --tests` で、`--ignored` を付けない。
偽の llama-server と偽の `Llm` だけを使い、本物の server には送らない。
陰性対照は、E の依頼から `with_system` を外す（`pipeline.rs` と `eval.rs`）か、`cli.rs` の `announce_when_warm` から `warmup::warm_up` を外すと、下の「E の依頼」と「初回の依頼」の行が落ちることである（実走して 11 本が落ち、戻して差分が元と一致した）。

| case | 入力 | 期待値 | 置き場 | test の名前 |
|---|---|---|---|---|
| E の依頼 | `Completion` に system を足す | `messages` が `[system, user]` の順で、system を持たない依頼は `[user]` のまま | `hayamimi-relay/src/llm.rs` | `a_system_message_goes_before_the_user_message`、`a_request_is_one_user_message_with_the_stop_marker` |
| E の依頼 | ja の原文に T が `Hello.` と答えた最終の経路 | T の依頼は user 1 つ。E の依頼は system と user の 2 つで、user は T と同じ文、system は下書きの注で始まり「Rewritten」を持たない | `hayamimi-relay/src/pipeline.rs` | `the_final_path_asks_t_in_one_message_and_e_in_two` |
| E の依頼 | 偽の llama-server に T と E を頼む | body の `messages` が T は user 1 つ、E は system と user で、E の user が T と同じ文、system に下書きがある。順の違いと、system の無い E と、下書きの違いを拒む陰性の case 付き | `hayamimi-relay/tests/cs25.rs` | `cs25_stages_run_in_order_t_e_v_once`、`cs25_a_wrong_order_or_an_edit_without_the_draft_is_refused` |
| E の依頼 | `eval` の E | E の body が system と user の 2 つで、user が T と同じ文（空白を含む）、system が下書きの注と calques の行を持つ | `hayamimi-relay/tests/r8f_eval.rs` | `eval_asks_e_with_ts_prompt_and_the_draft_in_a_system_message`、`a_request_names_the_language_in_one_user_message` |
| E の system | calques、`ja_style.toml`、対の外の言語 | calques の行が注の後ろに並ぶ。ja 向けは `Style:` と `Avoid` の行。対の外は注だけ。対は `edit_system` と同じ | `hayamimi-relay/src/pipeline.rs` | `the_calques_stand_at_the_end_of_the_english_edit_system`、`the_edit_system_carries_the_draft_and_the_japanese_style`、`the_edit_system_into_a_named_target_follows_the_pair_only_for_the_pair` |
| E の答え | 札を持たない訳（probe の e1 の答え 2 件） | trim して返す | `hayamimi-relay/src/edit_answer.rs` | `answers_to_the_card_form_request_are_returned_as_they_are` |
| 早い終わりの reason | `incomplete` と `too_short` の保留 | 両方を数え、それぞれ `reason` と読んだ声の長さを持つ。log の event は `reason`、`span_ms`、`min_ms` を持つ | `hayamimi-asr/src/pipeline.rs`、`hayamimi-asr/src/endpoint.rs`、`hayamimi/src/log.rs` | `a_hold_carries_its_reason_and_the_speech_it_was_read_from`、`holds_are_named_for_the_log`、`a_held_endpoint_event_names_the_reason_the_span_and_the_minimum_without_the_text`、`every_event_the_code_emits_has_a_message_in_both_tables` |
| 早い終わりの影の印 | `近藤さん。`、`独占配信。`、`本音とは。`、`これを。`、`さあ。`、閉じの記号の無い文、日本語以外 | 順に `honorific`、`noun`、`particle`、`particle`、`interjection`。記号が無くても種類は同じ。日本語以外は `other`。`noun` と `honorific` だけが影の規則で完結になり、声が 1 s に満たなければ `too_short`。それ以外の文と日本語以外は生きている規則と同じ答え。生きている規則は `独占配信。` を `incomplete` のまま持つ | `hayamimi-asr/src/endpoint.rs`、`hayamimi-asr/src/pipeline.rs` | `a_japanese_tail_is_told_by_the_form_before_the_closing_mark`、`a_tail_does_not_need_the_closing_mark_or_the_space_around_it`、`an_empty_body_and_a_language_but_ja_have_the_other_tail`、`only_a_noun_and_an_honorific_end_a_sentence_in_the_shadow_rule`、`tails_are_named_for_the_log`、`the_shadow_rule_finalises_a_noun_or_an_honorific_ending_that_the_live_rule_keeps`、`the_shadow_rule_keeps_every_other_ending_the_live_rule_keeps`、`the_shadow_rule_answers_as_the_live_rule_where_the_live_rule_finalises_or_does_not_judge`、`a_hold_carries_the_tail_and_what_the_shadow_rule_would_have_done` |
| 早い終わりの影の印 | `asr.endpoint.held` の行 | `tail` と `shadow_complete` を持ち、項目は `reason`、`span_ms`、`min_ms`、`tail`、`shadow_complete` だけ（文の項目が無い） | `hayamimi/src/log.rs` | `a_held_endpoint_event_names_the_tail_and_the_shadow_answer_and_no_sentence` |
| 確定の `plan` | 持った文を使った segment と、読み直した segment | `held` と `decode` と `rest` で `plan` が分かれ、`held_gap_ms` は読み直した時だけ（差 2243 sample は 140 ms）。final を作らない plan は種類を持たない。持った文との差は、使った時も使わない時も返る | `hayamimi-asr/src/endpoint.rs`、`hayamimi/src/log.rs` | `the_held_path_and_the_decode_path_are_named_apart`、`the_gap_is_noted_only_when_a_kept_sentence_was_read_again`、`a_plan_that_makes_no_final_names_no_kind`、`the_gap_to_a_kept_sentence_is_reported_whether_or_not_it_was_used`、`the_decided_event_names_the_plan_and_the_gap_of_a_segment_read_again` |
| 初回の依頼 | 偽の `Llm` | 依頼は 2 本で、1 本目が T の形（user 1 つ）、2 本目が E の形（system と user。user は 1 本目と同じ文で、system は `edit_system` が ja の原文に対して組む注）。どちらも `max_tokens` 8。答えれば `form` が `t`、`e` の info が 1 件ずつ。T が失敗しても E を送り、E が失敗すれば `form` が `e` の warn 1 件で返る。transport の失敗は cause を event に載せない | `hayamimi-relay/src/warmup.rs` | `the_first_requests_are_t_in_one_message_then_e_in_two`、`answered_requests_log_one_info_event_each_with_their_form_and_time`、`a_failed_t_does_not_keep_e_from_being_sent`、`a_failed_e_logs_one_warn_event_and_returns`、`a_transport_failure_keeps_the_cause_out_of_both_events` |
| 初回の依頼 | 偽の `Llm` と時間の予算 | 1 本目は予算の全部を、2 本目は 1 本目が使った残りだけを上限に受ける（予算から 1 本目の所要を引いた値の近くにあり、その値以下で、1 s を超えて下回らない）。残りが無ければ 2 本目は送らず、`form` が `e`、`detail` が `budget`、`duration_ms` が null の warn を出す。`warm_up` の予算は `TIMEOUT` である | `hayamimi-relay/src/warmup.rs` | `the_first_request_may_take_the_whole_budget_and_the_second_what_is_left`、`no_time_left_for_e_is_said_and_e_is_not_sent`、`warm_up_gives_the_first_request_the_timeout_of_the_first_requests` |
| 初回の依頼 | `announce_when_warm` | 初回の依頼が T、E の順に 2 本、`relay.serve.started` より前に出る。失敗しても `relay.serve.started` に進む。2 つの event は日本語と英語の文言を持つ | `hayamimi-relay/src/cli.rs` | `the_first_requests_go_out_once_each_before_the_relay_says_it_serves`、`first_requests_that_fail_do_not_stop_the_relay_from_saying_it_serves`、`serving_events_are_worded_in_both_locales` |

追加の行（初回の依頼の起動の経路）：偽の translator に `announce_after_first_request` から届く依頼を数える。置き場は `hayamimi-relay/tests/warmup_start.rs` で、偽の translator は `127.0.0.1:0` で立てる。
- 依頼は 2 本で、1 本目の `messages` は user 1 つ、2 本目は system と user の 2 つである。user は `warmup::prompt()` と等しく、system は `warmup::system()` と等しい。test の名前は `the_start_sends_t_as_one_user_message_then_e_as_a_system_and_a_user_message`。
- 2 本目だけが 500 を返しても、event は `translate.warmup.sent`、`translate.warmup.failed`、`relay.serve.started` の順に出る。test の名前は `a_failed_e_request_still_lets_the_relay_say_it_serves`。
- どの依頼にも 3 s かかる偽の translator（`REQUEST_TIMEOUT` の 1.5 s より長く、`warmup::TIMEOUT` の 10 s より短い。2 本の合計 6 s も 10 s より短い）でも、`translate.warmup.sent` が 2 件出て、`translate.warmup.failed` は出ない。test の名前は `a_translator_that_answers_in_three_seconds_still_warms_up_at_start`。陰性対照は 2 つある。`cli.rs` の接続を `HttpLlm::with_timeout(&live.llm, limit)` から `HttpLlm::new(&live.llm)` に替えると `translate.warmup.failed` が出て落ちる（実走した）。`warmup.rs` の `send_edit` が E の接続に渡す上限を固定の 1 ms にすると、この test と `the_first_request_may_take_the_whole_budget_and_the_second_what_is_left` の 2 本が落ちる（実走した。戻した後の sha256 は元と一致した）。

E の形の依頼の陰性対照は、`warmup.rs` の `warm_up_within` から E の依頼（`send_edit`）を外すことである。`warmup.rs` の 7 本、`cli.rs` の 2 本、`warmup_start.rs` の 3 本の、計 12 本が落ちる（実走した。戻した後の差分は元と一致した）。

**この節の設計にない物**：`consider` が `incomplete` を実際に数えることは、認識器の model が要る経路なので、この節の test は通らない。RP-32 の 3 run の log で `asr.endpoint.held` の `reason` が `incomplete` の行が在ることを数える（NOT VERIFIED）。

### 4.71 RP-73（RP-67 の合否が起点の置き方で決まっていない）

**未実装。** test の file も関数も repo に無い。実装の席が足す。
test：`native/hayamimi-eval/tests/rp73.rs` と `native/hayamimi-eval/src/manifest.rs`（`mod tests`。001〜023 と 049、050 の組の分類の行）。受入 test の名前は `rp73_the_l_verdict_is_the_same_on_utterances_whose_end_is_certain`（未実装、持ち主 L10）。実走の行（044〜048）だけが `#[ignore = "…"]` を持つ。`testdata/` を読む行（044〜047）の理由は `testdata/` が git の管理外（`.gitignore` の `testdata/`）であること、048 の理由は `docs/results/rp67/` の記録がまだ置かれていないことを書く。ほかの行は fixture と git の中の記録を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp73`（001〜023 と 049、050 は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --lib manifest::tests`）。実走の行はこれに `-- --ignored` を足し、`testdata/` の lat-ja と lat-en の wav と manifest、`docs/results/rp67/` の `e2e1.jsonl` と `e2e2.jsonl` を読む。
設計は D 3.1 の「起点の確かさ」、D 6 と 7 の RP-73、D 10 の H90〜H95 である。

**数え方**：

1. clip ごとに `speech_end` と同じ 320 sample の frame の音量（dB）を取り、10 パーセンタイルを静かな側 `q`、95 パーセンタイルを大きい側 `l` とする。
2. 高さ `f` の線は `max(q + f × (l − q), l − 25)`、声の終わりはその線を 5 frame 続けて超えた最後の区間の終わり（clip の長さで切る）である。`f` は 0.35、0.25、0.15、0.10 の 4 つで、0.35 が manifest の規則である。
3. **確か**：0.25、0.15、0.10 の終わりと 0.35 の終わりの差が 40 ms 以下（40 ms を含む。en_08 がちょうど 40 ms）で、0.35 の終わりの後の 15 frame（300 ms。clip の中に在る分）の frame の dB 値の算術平均（電力の平均ではない。電力で平均すると en_03 が 9.0 dB から 2.6 dB に変わる）が 0.35 の線より `drop` dB 以上低い。`drop` は 6 と 10 の 2 つで数え、2 つの組を並べる。
4. **切れ**：0.35 の終わりが clip の最後の frame の中に在る。確かの条件より先に見る。
5. **不確か**：確かでも切れでもない。
6. RP-67 の判定（D 6 の RP-67 の数え方）を、言語ごとに全発話と確かの発話だけで出す。`drop` の 2 つの値のどちらでも 2 つの判定が同じ時だけ `Ok` で、違えば `Err` で「組で割れた」と言語と `drop` の値を名指す。割れの原因を起点と断定しない（確かの発話だけの p95 は同じ記録で 1,222 ms（10 dB）から 1,531 ms（6 dB）まで揺れ、標本が少ないだけでも割れうる）。確かの発話に訳の取れた有効な標本が 0 件なら `Err`。組ごとの `L` の p50 と p95 は、この test の出力にだけ並べて書く。
7. **実装の順**：RP-73 は、RP-67 が初めて通ったと名乗る前に要る。それより前に、`L` を縮める仕事（D 10 の H93、VAD、訳の段）から cargo の席と load の窓を取らない。

関数の名前と引数の形は実装の席が決める。ただし case 単位の test の名前は、各行の「関数の末尾」と同じにし、何も付けない。

**分類の関数が返す物**（001〜023 と 049、050）：clip の sample 列を受け、低さの線 `drop` が 6 dB の時と 10 dB の時のそれぞれの組（確か、切れ、不確か）、高さ 0.35、0.25、0.15、0.10 の声の終わり（sample。clip の長さで切る）、0.35 の終わりの後ろの窓（clip の中に在る分。多くて 15 frame）の低さ（dB。窓が空なら無し）を返す。frame が無い clip と、線を 5 frame 続けて超える区間が無い clip（`speech_end` が `None`）は、組を返さない。

**切れの境界**：0.35 の終わりが clip の長さ − 320 より後ろなら切れ。ちょうど clip の長さ − 320 の時は切れではない（長さが 320 の倍数なら、最後の frame の音が線を超えていない場合である）。最後の frame が 320 sample に満たない clip では、終わりがその frame の頭でも切れ（頭は clip の長さ − 320 より後ろ）。切れは確かより先に見る。

**clip の書き方**（式で作る。実測のふりをしない）：`[n×A]` は、振幅 A（整数）の符号を交互にした sample を 320 個並べた frame を n 個。frame の dB は 10·log10(A² / 32768²)（A が 0 なら −120 dB）。`[1×A（100 sample）]` は 100 sample で終わる最後の frame。百分位の順位は `speech_end` と同じ (N − 1) × p / 100 の整数除算（0 始まり）。dB は振幅を整数に丸めた後の値で、表の dB は小数第 2 位まで書き、±0.05 dB の幅で読む。

- **D36**：q は振幅 164（−46.01 dB）、l は 10362（−10.00 dB）。線は 0.35 が −33.41 dB、0.25、0.15、0.10 が −35.00 dB（l − 25 の枝が式の枝より上）。帯 639（−34.20）は 0.35 の線の下で l − 25 の上。519（−36.01）は l − 25 の下で、0.25 の式の線（−37.01）の上。661（−33.91）は 0.35 の線の 0.5 dB 下。
- **D26**：q は 328（−39.99）、l は 6540（−14.00）。線は 0.35 が −30.89、0.25 が −33.49、0.15 が −36.09、0.10 が −37.39 dB で、4 つとも式の枝（l − 25 は −39.00 でその下）。帯 823（−32.00）は 0.25 と 0.35 の線の間、583（−35.00）は 0.15 と 0.25 の線の間、474（−36.79）は 0.10 と 0.15 の線の間、596（−34.80）は 0.15 と 0.25 の線の間。
- **K**：q は 1000（−30.31 dB）。l は行ごとの振幅。窓の低さは 0.35 × 20 × log10(l の振幅 / 1000) dB。

001〜023 の終わり、低さ、組は、frame の dB、百分位、線、5 frame の連なり、窓の平均を別に組んだ awk の式に当てて、手で出した値と一致した（pseudo、2026-10-22）。

**判定の行の標本**（024〜043、048）：標本は F67 の形（`d` 500 ms、`L_dec` 150 ms、訳が取れている）で、`L` を x ms にするには `L_tr` を x − 650 ms にする。ja は 23 発話 × 5 回、en は 15 発話 × 5 回。組の表は D 3.1 のとおり、ja の 6 dB は確か {1, 6, 8, 16}、切れ {10, 11, 13, 14, 19, 20, 21, 22}、不確か {2, 3, 4, 5, 7, 9, 12, 15, 17, 18, 23}、10 dB は確か {1, 6, 8}、切れは同じ、不確かは 6 dB の不確かに 16 を足した 12 発話。en の 6 dB は 15 発話が確かで、10 dB の確かは 12 発話（fixture では確かでない 3 発話を {2, 9, 14} と置く）。判定は RP-67 の `L` の p95 の判定（標本の絞り、nearest-rank、1600 ms の線）を、全発話の標本と、6 dB の確かの標本と、10 dB の確かの標本のそれぞれに掛け、3 つが同じ時だけ `Ok` を返す。順位は (95 × n + 99) / 100 の整数除算で、次の表のとおり。

| 標本の集合 | n | 順位 | 1600 ms を超えてよい数 |
|---|---|---|---|
| ja の全発話 | 115 | 110 | 5 |
| ja の 6 dB の確か | 20 | 19 | 1 |
| ja の 10 dB の確か | 15 | 15 | 0 |
| en の全発話（6 dB の確かと同じ） | 75 | 72 | 3 |
| en の 10 dB の確か（fixture） | 60 | 57 | 3 |

**組の分類**（001〜023 と 049、050。`manifest.rs` の `mod tests`）：

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-73-001 | D36 の `[20×164][30×10362][50×164]`（N 100、32,000 sample）。声の後ろは q の高さの静けさだけ | 確かの典型。高さを下げても終わりが動かず、後ろが十分低い | 動き ≤ 640 sample、低さ ≥ 6 dB と ≥ 10 dB | 4 高さの終わりは 16,000 が 4 つ（(20 + 30) frame × 320。derived、数え方の 2）。窓は 15 frame で、低さは 0.35 の線 −33.41 から窓の平均 −46.01 を引いた 12.60 dB。組は 6 dB でも 10 dB でも確か（derived、D 3.1 の確かの規則） | 陽性 | `a_clip_whose_voice_stops_dead_is_certain_at_both_lines` |
| T-RP-73-002 | D36 の `[20×164][30×10362][2×639][48×164]` | 動きがちょうど 40 ms（640 sample）。確かの側の境界 | 動き ≤ 640 sample（640 を含む） | 終わりは 16,000、16,640、16,640、16,640（帯 −34.20 は 0.25 以下の線 −35.00 を超え、0.35 の線 −33.41 を超えない）。動きは 640 sample。低さは 11.03 dB で両方の線を超えるので、確かでない理由になるのは動きだけ。組は 6 dB でも 10 dB でも確か（derived、D 3.1 の「40 ms を含む」、en_08 の形） | 境界、陽性 | `a_movement_of_exactly_40_ms_is_still_certain` |
| T-RP-73-003 | D36 の `[20×164][30×10362][3×639][47×164]` | 動きが 60 ms（960 sample）。確かでない側の境界 | 同上 | 終わりは 16,000、16,960、16,960、16,960。動きは 960 sample で 40 ms を 1 frame 超える。低さは 10.24 dB で両方の線を超えるので、理由は動きだけ。組は 6 dB でも 10 dB でも不確か。切れではない（0.35 の終わり 16,000 は clip の長さ − 320 = 31,680 より前）（derived） | 境界、陰性 | `a_movement_of_60_ms_is_not_certain_though_the_tail_is_low` |
| T-RP-73-004 | D26 の `[20×328][30×6540][3×474][47×328]`（帯 474 は 0.10 の線の上、0.15 の線の下） | 0.10 の高さだけが 60 ms 動く | 動き ≤ 640 sample | 終わりは 16,000、16,000、16,000、16,960。低さは 8.46 dB で 6 dB の線を超える。組は 6 dB で不確か（理由は動き 960 sample だけ）、10 dB でも不確か（derived、pseudo の awk の式と一致） | 境界、陰性 | `only_the_lowest_line_moving_60_ms_makes_the_clip_uncertain` |
| T-RP-73-005 | D26 の `[20×328][30×6540][3×583][47×328]`（帯 583 は 0.15 の線の上、0.25 の線の下） | 0.15 と 0.10 の高さが 60 ms 動く | 同上 | 終わりは 16,000、16,000、16,960、16,960。低さは 8.10 dB。組は 6 dB でも 10 dB でも不確か（理由は 6 dB では動きだけ）（derived、同上） | 境界、陰性 | `the_two_lowest_lines_moving_60_ms_make_the_clip_uncertain` |
| T-RP-73-006 | D26 の `[20×328][30×6540][3×823][47×328]`（帯 823 は 0.25 の線の上、0.35 の線の下） | 0.25、0.15、0.10 の高さが 60 ms 動く | 同上 | 終わりは 16,000、16,960、16,960、16,960。低さは 7.50 dB。組は 6 dB でも 10 dB でも不確か（理由は 6 dB では動きだけ）（derived、同上） | 境界、陰性 | `the_three_lower_lines_moving_60_ms_make_the_clip_uncertain` |
| T-RP-73-007 | D26 の `[20×328][30×6540][15×328][20×596][15×328]`（声の後ろに 300 ms の静けさ、その後に 400 ms の尾。尾 596 は静かな側から 0.20 の高さ） | 尾が窓の外に在る。窓の平均だけでは見えない | 動き ≤ 640 sample | 終わりは 16,000、16,000、27,200、27,200（0.15 と 0.10 の線 −36.09、−37.39 は尾 −34.80 に超えられ、尾の終わりは (20 + 30 + 15 + 20) frame × 320。0.25 の線 −33.49 は超えられない）。動きは 11,200 sample。窓は 15 frame が全部 q で、低さは 9.10 dB。組は 6 dB でも 10 dB でも不確か（derived、D 3.1 の「声の尾と背景の音が音量で分かれない」） | 陰性、境界。40 ms の安定の検査を外した build は、低さ 9.10 dB だけで 6 dB の組を確かと返して落ちる（10 dB の組は 9.10 < 10 で不確かのままなので、6 dB の組で見る） | `a_tail_beyond_the_window_makes_the_clip_uncertain` |
| T-RP-73-008 | D36 の `[20×164][40×10362]`（N 60、19,200 sample）。声が最後の frame まで続く | 切れの典型 | 0.35 の終わり > clip の長さ − 320 = 18,880 | 終わりは 19,200 が 4 つ（clip の長さ）。窓は空で低さは無し。組は 6 dB でも 10 dB でも切れ（specified、D 3.1 の切れ。確かの検査より先に見る） | 陽性。空の窓を「低い」と読んで確かを先に見る build は確かと返して落ちる | `a_voice_running_to_the_last_frame_is_cut` |
| T-RP-73-009 | D36 の `[20×164][39×10362][1×164]`（N 60）。声が最後の 1 frame 手前で止まる | 切れの 1 frame 手前。切れではない側の境界 | 同上 | 終わりは 18,880 が 4 つ（= clip の長さ − 320。切れの条件の等号）。窓は 1 frame で低さは 12.60 dB。組は 6 dB でも 10 dB でも確か（derived）。最後の frame が短い時は、終わりがその frame の頭でも切れ（lat-ja の発話 19、59,065 sample、終わり 58,880。049 が見る） | 境界、陰性。等号を切れに入れる build は切れと返して落ちる | `a_voice_ending_one_frame_before_the_end_is_not_cut` |
| T-RP-73-010 | D36 の `[20×164][39×10362][1×10362（100 sample）]`（N 60、18,980 sample）。最後の frame が 100 sample | clip の長さで切る終わり | 0.35 の終わり > 18,660（clip の長さ 18,980 − 320） | 終わりは 18,980 が 4 つ（(first + 5) × 320 = 19,200 を clip の長さ 18,980 で切る）。組は 6 dB でも 10 dB でも切れ（derived、D 3.1 と `speech_end` の `min(samples.len())`） | 境界、陽性。切らない build は 19,200 を返して落ちる | `a_last_frame_shorter_than_320_samples_ends_the_voice_at_the_clip_length` |
| T-RP-73-011 | D36 の `[20×164][30×10362][5×164]`（N 55）。窓に入る frame が 5 | 窓が 15 frame に満たない（clip の終わりで短い） | 低さ ≥ 6 dB と ≥ 10 dB | 終わりは 16,000 が 4 つ。窓は 5 frame で、低さは 12.60 dB（5 frame で割る）。組は 6 dB でも 10 dB でも確か（derived、数え方の 3 の「clip の中に在る分」） | 境界、陽性。5 frame の和を 15 で割る build は低さが負になり不確かと返して落ちる | `a_window_shorter_than_300_ms_is_averaged_over_its_own_frames` |
| T-RP-73-012 | K の `[20×1000][30×7173][5×1000]`（N 55）。窓は 5 frame で、低さは 5.99 dB | 窓が短く、6 dB に 0.01 dB 足りない | 低さ ≥ 6 dB | 低さは 0.35 × 20 × log10(7173 / 1000) = 5.98991 dB（derived）。終わりは 16,000 が 4 つ。組は 6 dB でも 10 dB でも不確か | 境界、陰性。足りない frame を −120 dB で埋めて 15 frame で割る build は低さが大きくなり確かと返して落ちる | `a_short_window_that_is_not_low_enough_is_not_padded_with_silence` |
| T-RP-73-013 | K の `[20×1000][30×7197][50×1000]`（窓 15 frame） | 低さが 6 dB の線のすぐ上（整数の振幅で作れる最も近い値） | 低さ ≥ 6 dB | 低さは 0.35 × 20 × log10(7197 / 1000) = 6.00006 dB（±0.001。derived）。終わりは 16,000 が 4 つ。組は 6 dB で確か、10 dB で不確か | 境界、陽性 | `a_tail_6_db_below_the_line_is_certain_at_the_6_db_line_only` |
| T-RP-73-014 | K の `[20×1000][30×7173][50×1000]` | 低さが 6 dB の線に 0.01 dB 足りない | 同上 | 低さは 5.98991 dB（±0.001。derived）。組は 6 dB でも 10 dB でも不確か | 境界、陰性 | `a_tail_0_01_db_short_of_6_db_is_not_certain` |
| T-RP-73-015 | K の `[20×1000][30×26828][50×1000]` | 低さが 10 dB の線のすぐ上 | 低さ ≥ 10 dB | 低さは 0.35 × 20 × log10(26828 / 1000) = 10.00012 dB（±0.001。derived）。組は 6 dB でも 10 dB でも確か | 境界、陽性 | `a_tail_10_db_below_the_line_is_certain_at_both_lines` |
| T-RP-73-016 | K の `[20×1000][30×26738][50×1000]` | 低さが 10 dB の線に 0.01 dB 足りない | 同上 | 低さは 9.98990 dB（±0.001。derived）。組は 6 dB で確か、10 dB で不確か | 境界、陰性 | `a_tail_0_01_db_short_of_10_db_is_certain_only_at_the_6_db_line` |
| T-RP-73-017 | D36 の `[20×164][30×10362]` に続けて `[1×164][1×661]` を 5 回、`[40×164]`（N 100）。窓の 15 frame は q の 10 frame と 661 の 5 frame（661 は孤立で、5 frame の連なりを作らない） | 窓の平均を dB で取るか、電力で取るか | 低さ ≥ 6 dB | 終わりは 16,000 が 4 つ。低さは dB 値の算術平均で 8.57 dB（(5 × 0.5 + 10 × 12.6) / 15）。電力で平均すると 4.76 dB。組は 6 dB で確か、10 dB で不確か（derived、D 3.1 の「電力で平均すると組が崩れる」。awk の式で 8.568 と 4.764） | 境界、陰性。電力で平均する build は 6 dB の組を不確かと返して落ちる | `the_tail_is_averaged_in_decibels_not_in_power` |
| T-RP-73-018 | D26 の `[20×328][30×6540][6×583][44×328]`（帯 583 を 6 frame） | 線の式の枝が l − 25 の枝より上（l − 25 は −39.00） | 線 = max(q + f × (l − q), l − 25) | 終わりは 16,000、16,000、17,920、17,920（(50 + 6) frame × 320。帯 −35.00 は 0.35 の線 −30.89 と 0.25 の線 −33.49 に届かず、0.15 の線 −36.09 と 0.10 の線 −37.39 を超える）。組は 6 dB でも 10 dB でも不確か（動き 1,920 sample）（derived） | 境界、陰性。l − 25 だけを線にする build は 4 つとも 17,920 を返して落ちる | `the_percentile_line_decides_when_it_is_above_the_25_db_floor` |
| T-RP-73-019 | D36 の `[20×164][30×10362][6×519][44×164]`（帯 519 を 6 frame） | l − 25 の枝が式の枝より上（0.25、0.15、0.10 の式の線は −37.01、−40.61、−42.41） | 同上 | 終わりは 16,000 が 4 つ（0.25、0.15、0.10 の線は l − 25 の −35.00 で、帯 −36.01 はどの線にも届かない。0.35 の線は −33.41）。低さは 8.60 dB。組は 6 dB で確か、10 dB で不確か（derived） | 境界、陰性。式の枝だけを線にする build は 0.25 の線を −37.01 にして終わりを 17,920 にし、落ちる | `the_25_db_floor_decides_when_the_percentile_line_is_lower` |
| T-RP-73-020 | `[20×164][6×10362][25×519][52×164]`（N 103） | 95 パーセンタイルの順位。(N − 1) × 95 / 100 = 96 番目（0 始まり）と、nearest-rank の 97 番目で値が分かれる | l = 並べ替えた列の 96 番目 | l は 519（−36.01 dB）。線は 0.35 が −42.51。終わりは 16,320 が 4 つ（(20 + 6 + 25) frame × 320。519 の 25 frame も超える）（derived、`speech_end` と同じ順位） | 境界、陰性。97 番目を使う build は l を 10362 にして線を −33.41 にし、519 を超えられず終わりを 8,320 にして落ちる | `the_loud_level_is_the_95th_percentile_by_integer_rank` |
| T-RP-73-021 | `[10×0][14×164][30×3277][6×328][40×164]`（N 100） | 10 パーセンタイルの順位。(N − 1) × 10 / 100 = 9 番目（0 始まり）と、N × 10 / 100 = 10 番目で値が分かれる | q = 並べ替えた列の 9 番目 | q は −120 dB（無音の 10 frame の最後）。線は l − 25 = −45.00 で、帯 328（−39.99）を超える。終わりは 19,200 が 4 つ（(10 + 14 + 30 + 6) frame × 320）（derived） | 境界、陰性。10 番目を使う build は q を −46.01 にして 0.35 の線を −36.91 にし、終わりを 17,280、17,280、19,200、19,200 にして落ちる | `the_quiet_level_is_the_10th_percentile_by_integer_rank` |
| T-RP-73-022 | 2 入力。(a) 0 sample の clip。(b) `[10×164][1×10362][10×164][1×10362][10×164][1×10362][10×164][1×10362][10×164]`（N 54。声の高さの frame が孤立していて、5 frame の連なりが無い） | 組を決められない clip | 終わりが在る | 2 入力とも組を返さない（`None` か `Err`。型は実装の席が決め、確か、切れ、不確かのどれも返さない）。4 高さの終わりも返さない（implicit、`speech_end` が `None` を返す形） | 陰性、境界。空の clip を不確かと返す build は落ちる | `a_clip_without_a_speech_run_has_no_group` |
| T-RP-73-023 | 001〜022 の clip のうち組を返す物 | 高さを下げると終わりは前へ動かない | 終わりの列が減らない | 各 clip で、0.35、0.25、0.15、0.10 の終わりが 0.35 ≤ 0.25 ≤ 0.15 ≤ 0.10 の順（partial、線を下げると超える frame の集合が広がるので、最後の連なりの終わりは前へ動かない） | property | `the_ends_never_move_earlier_as_the_line_goes_down` |
| T-RP-73-049 | D36 の `[20×164][30×10362]` に、164 の 185 sample の最後の frame を続ける（N 51、16,185 sample）。声は最後の frame の頭で止まる | 切れの境界。最後の frame が 320 sample に満たず、終わりがその frame の頭 | 0.35 の終わり > clip の長さ − 320 = 15,865 | 終わりは 16,000 が 4 つ（(20 + 30) frame × 320）。16,000 は 15,865 より後ろなので、組は 6 dB でも 10 dB でも切れ（specified、D 3.1 の切れ。derived、lat-ja の発話 19 の形：59,065 sample、終わり 58,880 は 58,880 > 58,745） | 境界、陽性。切れの基準を frame の数から出した 16,000 にする build は 16,000 > 16,000 が偽で切れでないと返し、窓の低さ 12.60 dB で確かと返して落ちる | `a_voice_ending_where_a_short_last_frame_begins_is_cut` |
| T-RP-73-050 | clip を通さず、分類の関数が返す読み（4 高さの終わりは 16,000、窓の低さはちょうど 6 dB、切れでない）を直に組む | 低さの線のちょうど（等号）。整数の振幅の clip では作れない値 | 低さ ≥ 6 dB（6 dB を含む）、低さ ≥ 10 dB | 6 dB の線で確か、10 dB の線で不確か（specified、数え方の 3 の「`drop` dB 以上低い」。derived、6.0 は 6 以上で 10 未満） | 境界、陽性、陰性。以上を超過にする build は 6 dB の線で不確かと返して落ちる | `a_tail_exactly_on_the_low_line_is_certain` |

**判定の行**（024〜043。`rp73.rs`。`--ignored` なしで通る）：

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-73-024 | ja は確か {1, 6, 8, 16} の 20 標本を `L` 900 ms、ほかの 95 標本を 1,700 ms。en は全標本 900 ms | 確かだけが線の内側（全発話は外側）。割れる | 全発話と確かの判定が同じ | `Err` で ja を名指し、「組で割れた」と 6 と 10 の両方の線を言う。en は名指さない。出力の p95 は全発話 1,700 ms（n 115、順位 110）、6 dB の確か 900 ms（n 20、順位 19）、10 dB の確か 900 ms（n 15、順位 15）（derived、nearest-rank。D 6 の RP-73 の陰性の 1 本目）。ほかの標本は設計の行（`rust_port.ja.md:2256`）と同じ 1,700 ms で、1,500 ms では全発話の p95 も 1,500 ms で 1600 ms の内側になり割れない（derived） | 陰性。確かの判定だけを返す build は `Ok` を返し、全発話の判定だけを返す build は「割れた」を言わず、どちらも落ちる | `a_verdict_that_splits_on_the_certain_utterances_names_the_language` |
| T-RP-73-025 | ja は確か {1, 6, 8, 16} の 20 標本を `L` 1,700 ms、ほかの 95 標本を 900 ms。en は全標本 900 ms | 3 つの判定が全部 `Err`（同じ）。割れていない | 同上 | `Ok`。出力に 3 つの p95 を 1,700 ms と書き、3 つとも 1600 ms の線の外側と書く（全発話は n 115 の順位 110 が 1,700 ms）。RP-73 の `Ok` は RP-67 の通過ではない（specified、D 6 の RP-73 の「2 つが同じなら `Ok`」） | 陽性、境界 | `an_agreed_failure_is_not_a_split` |
| T-RP-73-026 | ja と en の全標本を 900 ms | 全部が内側。割れない | 同上 | 両方 `Ok`。出力に組ごとの n、p50、p95 が並ぶ。ja は確か n 20、切れ n 40、不確か n 55（4、8、11 発話 × 5 回）、全部 p50 900 ms、p95 900 ms。en は確か n 75（specified、数え方の 6 の「組ごとの `L` の p50 と p95 は出力にだけ並べる」） | 陽性 | `all_samples_at_900_ms_agree_and_the_groups_are_written` |
| T-RP-73-027 | ja は切れの発話 10 の 5 標本だけ `L` 1,700 ms、ほかは 900 ms。en は全標本 900 ms | 組ごとの p95 が線の外側でも、判定の対には入らない | 同上 | `Ok`。全発話の p95 は n 115 の順位 110 が 900 ms、確かの 2 つも 900 ms。出力の切れの組は n 40、p50 900 ms、p95 1,700 ms（順位 38）。切れの p95 が 1600 ms を超えても合否に使わない（derived、数え方の 6 の「記録だけ」） | 陽性、境界。組ごとの p95 を判定に入れる build は `Err` を返して落ちる | `a_group_p95_over_the_line_is_written_and_does_not_decide` |
| T-RP-73-028 | ja は発話 1 の 1 回目だけ `L` 1,700 ms、ほかは 900 ms。en は全標本 900 ms | 標本が 1 件。10 dB の確か（n 15）だけが割れる | 同上 | `Err` で ja と 10 dB の線を名指す。6 dB は名指さない。全発話は n 115 の順位 110 が 900 ms、6 dB の確かは n 20 の順位 19 が 900 ms、10 dB の確かは n 15 の順位 15 が 1,700 ms（derived。D 6 の RP-73 の「標本が少ないだけでも割れうる」） | 陰性、境界。6 dB の確かだけを見る build は `Ok` を返して落ちる | `one_slow_sample_splits_the_10_db_set_alone` |
| T-RP-73-029 | 2 入力。ja で発話 16 の標本のうち 1 件（入力 a）と 2 件（入力 b）を `L` 1,700 ms、ほかは 900 ms。en は全標本 900 ms | 6 dB の確か（n 20、順位 19）の超えてよい数ちょうど。10 dB の確かは発話 16 を含まない | 同上 | a は `Ok`（6 dB の確かの 19 番目は 900 ms）。b は `Err` で ja と 6 dB の線を名指し、10 dB は名指さない（19 番目が 1,700 ms。全発話は n 115 の順位 110 が 900 ms）（derived） | 境界 | `the_6_db_set_of_20_samples_allows_one_over` |
| T-RP-73-030 | en で fixture の確かでない発話 {2, 9, 14} の 15 標本を `L` 1,700 ms、ほかの 60 標本を 900 ms。ja は全標本 900 ms | en の全発話と 6 dB の確か（15 発話）は `Err` で同じ。10 dB の確か（12 発話、n 60）だけ内側 | 同上 | `Err` で en と 10 dB の線を名指す。ja は名指さない。全発話は n 75 の順位 72 が 1,700 ms、6 dB の確かも同じ集合で 1,700 ms、10 dB の確かは n 60 の順位 57 が 900 ms（derived） | 陰性 | `a_split_in_en_is_named_alone` |
| T-RP-73-031 | 028 の ja と 030 の en を 1 つの入力にする | 両方の言語が割れる | 同上 | `Err` で ja と 10 dB の線、en と 10 dB の線の両方を名指す（derived、E2E 2 本の run を別々に判定する T-RP-67-012、013 の形） | 陰性 | `both_languages_are_named_when_both_split` |
| T-RP-73-032 | 2 入力。(a) ja の確か {1, 6, 8, 16} の標本の `t_tr_us` を null（訳なし）にする。(b) 同じ標本の `d` を 100 ms（終端不正）にする。ほかは 900 ms | 確かの発話に訳の取れた有効な標本が 0 件 | 件数 ≥ 1 | 2 入力とも `Err` で ja を名指し、訳の取れた有効な標本が 0 件であることを言う。6 dB と 10 dB の両方（specified、数え方の 6 の「0 件なら `Err`」。0 件を一致と読まない） | 陰性、境界。空の集合の p95 を 0 とみなして全発話と比べる build は落ちる | `a_certain_set_without_a_translated_valid_sample_is_an_error` |
| T-RP-73-033 | ja の確か {1, 6, 8} の標本だけ `t_tr_us` を null にする。発話 16 は 900 ms | 10 dB の確か（{1, 6, 8}）だけが 0 件。6 dB の確か（発話 16 の 5 標本）は在る | 同上 | `Err` で ja と 10 dB の線を名指し、0 件であることを言う。6 dB は名指さない（derived） | 陰性、境界 | `an_empty_10_db_set_names_the_10_db_line` |
| T-RP-73-034 | 024 の入力の `Err` の文 | 割れた時の文。原因を起点と断定しない | 文に言語、線、p95 と n を載せる | 文は「組で割れた」、ja、6 と 10、全発話の p95 1,700 ms と n 115、確かの p95 900 ms と n 20（6 dB）、n 15（10 dB）を含み、「誤り」の字を含まない（implicit、数え方の 6 の「割れの原因を起点と断定しない」。字の不在は近似） | 陰性 | `the_split_message_carries_the_counts_and_names_no_cause` |
| T-RP-73-035 | 一時 dir に個別の clip の wav を 3 本（001 の形、008 の形、007 の形）と manifest（`wav`、`start_sample`、`end_sample` を `concat` と同じ式で置く。間の無音は 24,000 sample）を作る。標本は発話 1、2、3 の各 5 回で、発話 2 だけ `L` 1,700 ms、ほかは 900 ms（ja の形） | 発話の番号と manifest の行の結び。発話 k は 0 始まりの k − 1 行目 | 同上 | 組の表は 1 が確か、2 が切れ、3 が不確か（derived、001、008、007 の組）。`Err` で ja と 6 dB と 10 dB の両方を名指す（全発話は n 15 の順位 15 が 1,700 ms、確かの発話 1 は n 5 で 900 ms）（derived） | 境界、陰性。発話 k を k 行目に結ぶ build は確かの集合が空になり、「0 件」の `Err` を返して落ちる | `utterance_k_is_the_manifest_row_k_minus_1` |
| T-RP-73-036 | 035 の manifest と wav（各行の `end_sample` は `start_sample` + `speech_end`。start は 0、56,000、99,200、end は 16,000、75,200、115,200） | 全発話で当て直した値が `end_sample` と一致 | 一致 | `Ok`。一致しない発話は 0 件（derived、`concat` の式。specified、数え方の 1 と D 6 の RP-73 の 1） | 陽性 | `a_manifest_made_by_the_join_matches_the_rule_again` |
| T-RP-73-037 | 036 の manifest で発話 1 の `end_sample` を 4,800 sample（300 ms）小さくする（11,200） | 確かの 1 発話の `end_sample` が 300 ms 前 | 一致 | `Err` で発話 1 を名指し、manifest の 11,200 と当て直した 16,000 を言う。発話 2 と 3 は名指さない（specified、D 6 の RP-73 の陰性の 2 本目） | 陰性 | `a_manifest_end_moved_300_ms_earlier_is_named_by_its_utterance` |
| T-RP-73-038 | 036 の manifest で発話 2 の `end_sample` を 1 sample 大きくする（75,201） | 1 sample のずれ。許容は無い | 一致 | `Err` で発話 2 を名指す（specified、「一致しない発話が在れば」。等号で比べ、幅を持たせない） | 境界、陰性 | `a_manifest_end_one_sample_off_is_named` |
| T-RP-73-039 | 036 の manifest で発話 1 と 3 の `end_sample` を 320 sample ずつ小さくする | 2 発話が一致しない | 一致 | `Err` で発話 1 と 3 の両方を名指す。発話 2 は名指さない（derived。1 本目で止めない） | 陰性 | `every_utterance_whose_end_differs_is_named` |
| T-RP-73-040 | 記録（git の中）：`docs/results/rp32/rs1.jsonl`〜`rs3.jsonl` を合わせた ja の標本と、ja の組の表（6 dB と 10 dB） | 実物の記録。割れない | 同上 | `Ok`。全発話の p95 は 1,460.791 ms（n 291）、6 dB の確かは 1,491.084 ms（n 60）、10 dB の確かは 1,222.845 ms（n 45）で、3 つとも 1600 ms の内側。組ごとは、確か n 60 の p50 739.306 ms と p95 1,491.084 ms、切れ n 84 の 748.865 ms と 1,370.740 ms、不確か n 147 の 908.165 ms と 1,425.901 ms（pseudo、`jq`。有効は `d` ≥ 120 ms、nearest-rank。2026-10-22 に打ち、D 6 の RP-73 の値と一致した） | 陽性 | `the_recorded_rp32_runs_do_not_split_on_ja` |
| T-RP-73-041 | 記録（git の中）：`docs/results/e2e/e2e1/e2e1.jsonl`（今の main の ja の run）と ja の組の表 | RP-67 が落ちる記録。3 つの判定が同じ | 同上 | `Ok`。全発話の p95 は 1,871.715 ms（n 93）、6 dB の確かは 1,777.552 ms（n 20）、10 dB の確かは 1,813.440 ms（n 15）で、3 つとも `Err`（1600 ms の外側）。出力にその 3 つの `Err` を書き、RP-73 の `Ok` が RP-67 の通過ではないことを示す。組ごとは、切れ n 24 の p50 1,277.621 ms と p95 1,458.034 ms、不確か n 49 の 1,444.076 ms と 1,925.482 ms、確か n 20 の p50 1,476.481 ms（pseudo、`jq`、同上。T-RP-67-014 の n 93 と 1,871.7 ms に一致） | 陽性 | `the_recorded_main_ja_run_fails_alike_on_every_set` |
| T-RP-73-042 | 記録（git の中）：`docs/results/e2e/e2e2/e2e2.jsonl`（今の main の en の run）と、en の 6 dB の組（15 発話が確か） | 6 dB の確かが全発話と同じ集合 | 同上 | `Ok`。全発話の p95 は 1,303.167 ms（n 70）、6 dB の確かも同じ集合で 1,303.167 ms（n 70）、p50 970.333 ms（pseudo、`jq`、同上。T-RP-67-014 の 1,303.2 ms に一致）。10 dB の確かは組の分類が要るので 047 が見る | 陽性 | `the_recorded_main_en_run_is_the_same_on_all_and_on_the_certain_set` |
| T-RP-73-043 | 2 入力。(a) 存在しない dir `tests/fixtures/no-such-rp73-records`。(b) `e2e1.jsonl` だけを置いた一時 dir | 記録が無い dir。1 本だけ無い dir | 2 本とも在る | a は `Err` で、記録が無いこと（`no record` で始まる文。rp61 の `a_directory_with_no_record_is_named_and_not_passed` の形）と、無い file の名前 `e2e1.jsonl` と `e2e2.jsonl` の両方を言う。b は `Err` で `e2e2.jsonl` だけを名指す（specified、実走の行は記録が無い時に通さない。implicit、0 件を一致と読まない） | 陰性。両方在る dir を渡すと読み込みに進む（陽性対照。026 の標本を jsonl に書いて渡す） | `a_directory_with_no_rp73_record_is_named_and_not_passed` |

**実走の行**（044〜048。`#[ignore = "…"]`。記録が置かれるまでは走らせない）：

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-73-044 | 実走：`testdata/manifests/lat-ja.json` と `lat-en.json`、各行の `wav` が指す個別の clip の wav | 実物の clip の組。3 つの規則を 38 発話に当てる | 001〜023 の規則 | ja の 6 dB は確か 4（発話 1、6、8、16）、切れ 8（10、11、13、14、19、20、21、22）、不確か 11（2、3、4、5、7、9、12、15、17、18、23）。ja の 10 dB は確か 3（1、6、8）、切れ 8（同じ）、不確か 12。en の 6 dB は確か 15、10 dB は確か 12（残る 3 発話の番号は設計に無い）（specified、D 3.1。転記、H94 の awk と `astats` の値。この行が Rust の関数との一致を初めて確かめる） | 陽性 | `the_lat_ja_and_lat_en_clips_fall_in_the_recorded_groups`（未実装、持ち主 L10） |
| T-RP-73-045 | 実走：044 と同じ manifest と clip の wav | 38 発話で当て直した値が manifest と一致 | 一致 | 各発話で `start_sample` + `speech_end`（個別の clip）が `end_sample` と 0 sample の差で一致し、一致しない発話は 0 件（specified、D 6 の RP-73 の 1。転記、H94 は同じ frame で 35 % の規則を当て直して 38 発話が 0 ms の差で再現した） | 陽性 | `every_manifest_end_sample_is_the_end_the_rule_gives_again`（未実装、持ち主 L10） |
| T-RP-73-046 | 実走：044 の `lat-ja.json` の写しで、発話 1 の `end_sample` を 4,800 sample 小さくする | 実物の確かの 1 発話の `end_sample` が 300 ms 前 | 一致 | `Err` で発話 1 を名指し、ほかの発話は名指さない（specified、D 6 の RP-73 の陰性の 2 本目。037 の実物版） | 陰性 | `a_real_manifest_end_moved_300_ms_earlier_is_named`（未実装、持ち主 L10） |
| T-RP-73-047 | 実走：`docs/results/e2e/e2e2/e2e2.jsonl` と、044 で分類した en の組（10 dB の確か 12 発話） | 実物の en の 10 dB の確か | 同上（判定が同じ） | `Ok`。10 dB の確かの p95 は 1,271 ms 前後（n 60）で、全発話の 1,303.167 ms（n 70）と同じく 1600 ms の内側（転記、D 6 の RP-73 の値。丸めた ms なので 1 ms の幅で読む） | 陽性 | `the_recorded_en_run_is_the_same_on_the_12_utterances_certain_at_10_db`（未実装、持ち主 L10） |
| T-RP-73-048 | 実走：`docs/results/rp67/` の `e2e1.jsonl`（23 発話 × 5 回）と `e2e2.jsonl`（15 発話 × 5 回）、044 の組 | 窓 E4 の記録。受入 | 全発話と確かの発話だけで判定が同じ | 両言語で、全発話、6 dB の確か、10 dB の確かの判定が同じで `Ok`。出力に組ごとの n、p50、p95 が並ぶ（specified、6 の RP-73。確かの発話に訳の取れた有効な標本が在る） | 陽性 | `rp73_the_l_verdict_is_the_same_on_utterances_whose_end_is_certain`（未実装、持ち主 L10） |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- D 6 の RP-73 の陰性の 1 本目「確かの標本だけを `L` 900 ms、ほかを 1,500 ms」は、1,500 ms では全発話の p95 も 1,500 ms で線の内側になり割れない（024）。同じ行の括弧は「全発話で `Err`、確かだけで `Ok`」と書くので、024 は 1,700 ms に置いた。ほかの値で置き直す時は D の行を直す。設計の行へ写した（`rust_port.ja.md:2256`）。
- D 6 の RP-73 の陽性の括弧「RP-67 の側はどれも `Err`」の指す物。040〜042 は、RP-67 の `L` の p95 の判定（標本の絞り、nearest-rank、1600 ms の線）だけを比べる形で書いた。記録の dir に `<run>.machine.jsonl` が無い `docs/results/rp32/` と `docs/results/e2e/` を読むので、RP-83 の機械の門（T-RP-67-019）は RP-73 の判定に入れていない。括弧が機械の門を指すのでなければ、この前提は要らない。設計の行へ写した（`rust_port.ja.md:2256`）。
- D 6 の RP-73 は `testdata/lat-ja.wav` と `lat-en.wav` を読むと書く。manifest の行の `end_sample` は `start_sample` + `speech_end`（個別の clip）で、つないだ wav から clip の区間を切り出すと、間の無音（−120 dB）が 10 パーセンタイルを動かして組が H94 の値と変わりうる。この節は各行の `wav` が指す個別の clip を読む形で書いた（044、045）。
- 安定の幅の「clip の長さで切った終わりで差が 641 sample になる」行は作れない。差が 640 sample を超えると、0.35 の終わりの後ろの窓は全部が 0.25 の線以上の帯の中になり、0.35 の線との差が 0.10 × (l − q) 以下（最大でも約 3.85 dB）で、低さが 6 dB に届かず、40 ms の検査だけを分けられない。clip の長さで切る終わりそのものは 010 が見る。
- 低さの線のちょうど（等号）は、整数の振幅の dB が丸まるので作れない。clip では作れない。読みを直に組む T-RP-73-050 が見る（`a_tail_exactly_on_the_low_line_is_certain`）。013 と 015 は等号を分けず、分けるのは 050 である。013（6.00006 dB）と 015（10.00012 dB）が線の上の最も近い値、014 と 016 が 0.01 dB 足りない値で、等号側は両側から挟む。
- en の 10 dB で確かでない 3 発話の番号は設計に無い。030 の {2, 9, 14} は fixture の置き方で、実物の 3 発話は 044 が数で見る（NOT VERIFIED）。
- 「割れの原因を起点と断定しない」の文の検査（034）は、「誤り」の字の不在で近似した。
- 組を決められない clip の返し方の型（`None` か `Err`）。022 は組が返らないことだけを見る。設計の行へ写した（`rust_port.ja.md:2256`）。
- 007 の尾（静かな側から 0.20 の高さを 400 ms）の置き方。D 6 の RP-73 の陰性の 3 本目は窓の外の尾と読み、300 ms の静けさの後に 400 ms の尾を置いた。窓の内側に尾を置くと低さの検査が先に落ち、40 ms の検査を分けられない。

### 4.72 VAD の窓ごとの確率の probe

判定の code は変えない。
VAD の model を `ort` で開き、512 sample の窓ごとの確率を `lat-ja.wav` と `lat-en.wav` で出す。
sherpa-onnx の C API は確率を返さないので、この probe だけが窓ごとの確率を出す。
実走の command は `cd native && cargo test -j 2 --release -p hayamimi-asr --test vad_trace_probe -- --ignored --nocapture` で、出力は行の頭の語（`MODEL`、`GAP`、`NOISE`、`TRACE`、`FALL`、`CONTROL`）で分かれる。

| case | 入力 | 期待値 | 置き場 | test の名前 |
|---|---|---|---|---|
| 実走 | `models/silero_vad.onnx`、`lat-ja.wav`、`lat-en.wav` | `MODEL` の行に tensor の名前と形と layout が出る。0 だけの 20 秒の流れで確率が 0.5 を超えない（超えれば Err）。元の音と、manifest の終わりの後の 0 の連なりを -60 dBFS の白色雑音に替えた音の両方で、発話ごとの `FALL` の行が出る | `hayamimi-asr/tests/vad_trace_probe.rs` | `vad_window_probabilities`（`#[ignore]`） |
| layout | tensor の名前 | `x`、`h`、`c` と `prob`、`new_h`、`new_c` を LSTM の layout とする。ほかの 2 つの layout は名前で見分け、流さずに Err にする | 同上 | `the_lstm_names_give_the_lstm_layout`、`the_state_only_names_give_the_state_only_layout`、`the_rate_and_lstm_names_give_their_own_layout`、`other_names_give_no_layout` |
| 雑音 | 0 の連なり | 512 sample 以上の 0 の連なりだけを置き換え、短い連なりと発話の中は変えない。雑音の RMS が -60 dBFS の 0.2 dB 以内 | 同上 | `only_long_zero_runs_inside_a_gap_become_noise`、`zero_runs_shorter_than_the_minimum_are_left_alone`、`noise_has_the_requested_level` |
| 位置 | 窓の確率と manifest の終わり | 落ちる位置は、終わり以後に始まる最初の窓の開始から manifest の終わりを引いた ms。窓の置き場（発話の中か後か）は、直前に始まった発話で決める | 同上 | `a_fall_is_counted_from_the_manifest_end`、`a_window_is_placed_by_the_utterance_that_started_last` |
| 入力 | 順序の崩れた manifest | 順序の崩れ、重なり、入力の外を Err にする | 同上 | `spans_out_of_order_or_outside_the_input_are_refused` |

`--ignored` の無い run では、実走の 1 本を除く 10 本が通る。

### 4.73 RP-80（CI が cargo を打ち、Python を打たず、未実装の行の数を毎回出す）

test：`native/hayamimi-eval/tests/rp80.rs`。受入 test の名前は `rp80_ci_runs_cargo_and_no_python`。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp80`。実走の行（018、019）はこれに `-- --ignored` を足す。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない（017 だけは受入 test の名前の全部）。fixture は `native/hayamimi-eval/tests/fixtures/rp80_*/` の dir に置き、`include_str!` で渡す。
設計は D 6 の RP-80 と、8 の「R11 までの組」の R11c の行と CI の command の小節、D 10 の H103 である。

`check_workflow(workflow_yml, ci_probe_md, test_binaries)` は 3 つを確かめる(下の 5 を足すと 4 つ)。
`workflow_yml` は `.github/workflows/test.yml` の text、`ci_probe_md` は `docs/results/r11/ci_probe.md` の text、`test_binaries` は `native/<crate>/tests/*.rs` の（crate の名前、file の拡張子を除いた名前）の対の集合である。

1. **語の検査**：各行から数の step の test の名前 `rp19_python_cases_are_ported` の 1 つだけを除き、残りが `python|pip |pytest|setup-python`（設計の `grep -c -E` の式）に当たる行が 0。除く名前はこの 1 つだけで、ほかの名前の中の `python` は除かない。コメントの行も数える（grep は行の種類を見ない）。
2. **外した binary の検査**：test の step が `--test <名前>` で並べた binary の集合を U とする（`--test` が無ければ全部）。`--exclude <crate>` で外した crate の binary も外した物に数える。外した binary は 1 本ずつ `ci_probe.md` に名前と理由を持ち、理由は `testdata/` か `models/` を読むことである。clone で落ちる原因はこのほかに 3 つ在り、binary を外す理由にはしない（`docs/results/r11/ci_probe.md`）。`rp17` が compile の時に `env!` で `SHERPA_ONNX_LIB_DIR` を読むことは、先の step が `cargo build --release -p sherpa-onnx-sys` で変数を渡して扱う。`hayamimi-asr` の 4 本の unit test が `libonnxruntime.so.1` を開くことは、test の名前の `--skip` で扱う。`--test` を並べると cargo が lib、bin、example の target を選ばなくなることは、`--lib --bins --examples` で扱う。
3. **未実装の数の step**：`cargo test --release --jobs 2 -p hayamimi-eval --test rp19 -- --exact rp19_python_cases_are_ported` を打つ step が在り、その step が `continue-on-error: true` を持つ。
4. **例外の数の step**：3 の隣に、同じ形で `cargo test --release --jobs 2 -p hayamimi-eval --test rp81 -- --exact rp81_owned_exceptions_are_gone` を打つ step が在り、その step も `continue-on-error: true` を持つ（持ち主ごとの RP-81 の例外の数を log に出す）。
5. **くっついた旗の検査**：step の `run:` の値(`parse_steps` が返す。行頭か `- ` の直後の `run:` の後ろ)に、`--test`、`--skip`、`--exact`、`--ignored`、`--include-ignored` が、直前に半角空白か tab の無い形で現れたら落とす。値を半角空白と tab で語に切り、旗の綴りが語の先頭以外に在る語が、くっついた語である。tab は空白に数える(shell が語を切る字だから)。U+00A0 のような別の空白は語を切らないので、くっつきに数える。数え方は `grep -E 'run:' <test.yml> | grep -oE '[^[:space:]]--(test|skip|exact|ignored|include-ignored)'` の件数で、0 なら通る(今の file では実装と同じ件数になる。`run:` を含む別の行や複数行の値は式と実装で読みが違うので、実装を正とする)。値の先頭の旗、行頭の `--`、`run:` の値でない行(コメントや `name:` の文)は見ない。落とす時は、`run:` の行番号とくっついた語(空白で切った 1 語のまま)を名指す。

**F80**（基準の fixture）：`test.yml` は matrix（`windows-latest`、`ubuntu-latest`）の job に、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --jobs 2 -- -D warnings`、`cargo test --release --jobs 2 -p hayamimi-eval --test rp02 --test rp19 --test rp80 --test rp81` の step と、3 の step と 4 の step（どちらも `continue-on-error: true`）を持つ。`test_binaries` は `rp02`、`rp03`、`rp19`、`rp80` で、外した binary は `rp03` の 1 本。`ci_probe.md` は `rp03` の行に「`testdata/` を読む」の理由を持つ。python の語を含む行は 0。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-80-001 | F80 そのまま | 正しい workflow | 3 つとも成立 | `Ok`（specified、D 6 の RP-80 の期待値）。以後の陰性の行は、この fixture から 1 か所だけ変える | 陽性 | `a_workflow_that_runs_cargo_and_counts_the_unported_rows_passes` |
| T-RP-80-002 | R11 の前の `test.yml`（`rp80_before_r11/test.yml` に複製。18 行） | 検知の道具の陽性対照。語に当たる行が在る | 当たる行 4 | `Err` で 14、16、17、18 行目を名指す（`setup-python`、`python-version`、`pip install`、`python -m pytest`）。pseudo：`rg -c -e 'python\|pip \|pytest\|setup-python' .github/workflows/test.yml` が 4（実測 2026-10-20 11:4x。同じ式を `nainaimono_zzz` に替えると出力なしで rc 1）（specified、D 6 の RP-80 の陽性対照「R11 の前の `test.yml` で grep が 4」） | 陽性 | `the_workflow_before_r11_has_four_python_lines_and_is_rejected` |
| T-RP-80-003 | F80 に `- run: python -m pytest tests -q` の step を足す | `pytest` の step を残した形 | 当たる行 0 | `Err` でその行を名指す（specified、D 7 の RP-80 の陰性の case「`pytest` の step を残した workflow」） | 陰性 | `a_workflow_that_keeps_a_pytest_step_is_rejected` |
| T-RP-80-004 | F80 に次の 3 行を別々に足した 3 つの workflow：`- run: pytest -q`、`- run: pip install x`、`- run: python3 x.py` | 式の選択肢ごとに 1 つ。どれも他の選択肢に当たらない形 | 当たる行 0 | 3 つとも `Err` でその行を名指す（specified、式の `pytest`、`pip `、`python`） | 陰性 | `each_python_word_alone_is_rejected` |
| T-RP-80-005 | F80 に `shell: bash -o pipefail {0}` を足す | `pip` で始まる語（`pipefail`）。式の `pip ` は後ろの空白まで要る | 当たる行 0 | `Ok`（derived、式の `pip ` は空白を含む。検知の道具が語の途中に当たらない陰性対照） | 境界、陽性 | `a_word_that_only_starts_with_pip_is_not_python` |
| T-RP-80-006 | F80 に `# no python here` の行を足す | コメントの行。語は在る | 当たる行 0 | `Err` でその行を名指す（derived、grep は行の種類を見ないので、設計の「grep が 0」は注釈の語も許さない） | 境界、陰性 | `a_comment_that_names_python_is_rejected_like_any_other_line` |
| T-RP-80-007 | F80 から 3 の step を取り除く | 未実装の数の step が無い | step 1 本 | `Err` で数の step が無いことを言う（specified、D 7 の RP-80 の陰性の case「未実装の数の step を持たない workflow」） | 陰性 | `a_workflow_without_the_count_step_is_rejected` |
| T-RP-80-008 | F80 から 3 の step を取り除き、test の step の末尾に `-- --skip rp19_python_cases_are_ported` を足す | 数を消して緑にした形 | step 1 本 | `Err` で数の step が無いことを言う。`--skip` だけでは通らない（specified、D 7 の RP-80 の陰性の case「`--skip` に書いて数の step を置かない workflow」） | 陰性 | `a_skip_without_the_count_step_is_rejected` |
| T-RP-80-009 | F80 の 3 の step から `continue-on-error: true` を取り除く | 数の step が落ちて job を止める形 | `true` | `Err` で `continue-on-error` が無いことを言う（specified、D 6 の RP-80 の期待値「落ちても job を止めず」） | 陰性 | `a_count_step_that_stops_the_job_is_rejected` |
| T-RP-80-010 | F80 の 3 の step の `--exact` の名前を `rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks` に替える | 別の test を指す。`continue-on-error` は在る | 名前 `rp19_python_cases_are_ported` | `Err` で名前の食い違いを言う（specified、D 6 の RP-80 の期待値の command。slice ごとの数を出すのは `rp19_python_cases_are_ported` だけである） | 境界、陰性 | `a_count_step_that_runs_another_test_is_rejected` |
| T-RP-80-011 | F80 の `ci_probe.md` から `rp03` の行を取り除く | 外した binary が `ci_probe.md` に無い | 1 本ずつ名前 | `Err` で `rp03` を名指す（specified、D 7 の RP-80 の陰性の case「`ci_probe.md` に無い名前を挙げて落ちる」） | 陰性 | `an_excluded_binary_missing_from_the_probe_note_is_named` |
| T-RP-80-012 | F80 の `ci_probe.md` の `rp03` の行の理由を空にする | 名前は在り、理由が無い | 理由 1 つ | `Err` で `rp03` を名指す（specified、D 6 の RP-80 の期待値「名前と理由」） | 境界、陰性 | `an_excluded_binary_without_a_reason_is_named` |
| T-RP-80-013 | F80 の `ci_probe.md` の `rp03` の理由を「遅いので外す」にする | 理由が在るが `testdata/` も `models/` も読まない | `testdata/` か `models/` | `Err` で `rp03` を名指す（specified、D 6 の RP-80 の期待値「外す理由の無い binary を外さない」。外す理由は 8 の R11c の行の `testdata/` か `models/` を読むこと） | 境界、陰性 | `an_excluded_binary_with_another_reason_is_named` |
| T-RP-80-014 | F80 の `ci_probe.md` の `rp03` の理由を、`testdata/` を含む文と、`models/` を含む文に替えた 2 つ | 理由の 2 つの種類。013 の隣 | どちらか | どちらも `Ok`（specified、同上） | 境界、陽性 | `an_excluded_binary_with_a_testdata_or_models_reason_is_accepted` |
| T-RP-80-015 | F80 の test の step を `--test` の並びから `--workspace --exclude hayamimi-eval` に替える | crate 単位で外した形。`hayamimi-eval` の binary 4 本が外れる | 外した binary 1 本ずつ | `Err` で `ci_probe.md` に無い `rp02`、`rp19`、`rp80` を名指す（`rp03` は在る）（derived、D 6 の RP-80 の「外した test の binary は 1 本ずつ」。crate ごと外せば 1 本ずつの言い逃れになる） | 境界、陰性 | `an_excluded_crate_hides_no_binary_from_the_probe_note` |
| T-RP-80-016 | F80 の `rp19` の行を `test_binaries` から除く（`native/hayamimi-eval/tests/rp19.rs` が無い形） | 数の step が指す test の file が無い | file が在る | `Err` で `rp19` が無いことを言う（derived、3 の step の `--test rp19` が空振りする。`cargo` は存在しない test の指定で落ちるが、`continue-on-error` がそれを隠す） | 境界、陰性 | `a_count_step_without_its_test_file_is_rejected` |
| T-RP-80-017 | 受入：repo の `.github/workflows/test.yml` と `docs/results/r11/ci_probe.md`、`native/*/tests/*.rs` の実物 | 全体。001 から 016 と 020、021、022 から 030 の判定を実物に掛ける | 3 つとも成立 | `Ok`。pseudo：`sed 's/rp19_python_cases_are_ported//g' .github/workflows/test.yml \| grep -c -E 'python\|pip \|pytest\|setup-python'` が 0（名前を除かない数は 2 で、2 行とも数の step の名前。2026-10-20、`ddacac4`）。R11 の前の木（`archive/python-before-r11`）では同じ式で 4（002） | 陽性、陰性 | `rp80_ci_runs_cargo_and_no_python` |
| T-RP-80-018 | 実走：`git clone --no-local <repo> ~/.cache/hayamimi-r11/ci-probe`（`testdata/` と `models/` の無い clone）で、workflow の `run:` の行を同じ順に打つ | 実走。clone に `testdata/` も `models/` も無い | 全部 rc 0 | clone に `ls -d testdata models` が rc 2 で両方無いことを先に確かめ、`run:` の行が全部 rc 0（specified、D 6 の RP-80。worktree は本体の `testdata/` を引くので使わない）。cargo の席は同時 2 席までで、`--jobs 2` | 陽性 | `the_run_lines_pass_in_a_clone_without_testdata_or_models` |
| T-RP-80-019 | 実走：018 の clone で、`ci_probe.md` が挙げる binary を 1 本だけ単独で打つ（`cargo test --release --jobs 2 -p hayamimi-eval --test rp03`） | 検知の道具の陽性対照。外した理由が本当に落ちる | rc が 0 でない | rc が 0 でなく、落ちた理由が `testdata/` か `models/` の無いことを言う（specified、D 7 の RP-80「clone で落ちる binary」。外した理由が偽なら 018 は何も外せていない） | 陽性、陰性 | `an_excluded_binary_really_fails_in_a_clone_without_testdata_or_models` |
| T-RP-80-020 | F80 から 4 の step を取り除く | 例外の数の step が無い | step 1 本 | `Err` で例外の数の step が無いことを言う（specified、D 6 の RP-80 の陰性の case「RP-81 の例外の数の step を持たない workflow」）。3 の step は在るので、数の step が 1 本在るだけでは通らない | 陰性 | `a_workflow_without_the_owned_exceptions_step_is_rejected` |
| T-RP-80-021 | F80 の 4 の step から `continue-on-error: true` を取り除いた物と、4 の step の `--exact` の名前を `rp81_no_document_names_a_removed_python_file` に替えた物 | 4 の step の 2 つの崩し方。003 から 010 の 3 の step の対 | 在る | 2 つとも `Err`（specified、D 6 の RP-80「その隣に、同じ形で」。同じ形は `continue-on-error` と `--exact` の名前の両方） | 境界、陰性 | `an_owned_exceptions_step_in_a_wrong_form_is_rejected` |
| T-RP-80-022 | 2 入力。(a) F80 そのまま。(b) repo の `.github/workflows/test.yml` の実物(今の main の形。`-- --skip a --skip b` のように、旗の直前が全部空白) | 正しい workflow。旗が全部空白の後ろに在る | くっついた旗の件数 0 | どちらも検査 5 は通る(specified、RP-80 に足す条件。pseudo：`grep -E 'run:' .github/workflows/test.yml \| grep -oE '[^[:space:]]--(test\|skip\|exact\|ignored\|include-ignored)' \| wc -l` が 0。同じ file の `run:` を含む行は 11 行(`grep -cE 'run:'`)で、検査が空の入力を見ているのではない。2026-10-21 に打った) | 陽性 | `a_workflow_with_every_flag_after_a_space_passes_the_glued_flag_check` |
| T-RP-80-023 | F80 の test の step の末尾を `-- --skip rp19_python_cases_are_ported --skip an_engine_has_no_recognizer_to_warm--skip with_no_stage` にする(`--skip` が前の名前にくっつく) | `--skip` が前の名前にくっついた形 | くっついた旗 0 | `Err` で `warm--skip` を名指す(specified、同上。件数は 1。`--skip` を空白で分けて数える検査は、`warm--skip` を 1 語と読み、旗と見ずに通すので、ここで落ちる。022 が陽性対照) | 陰性 | `a_skip_glued_to_the_previous_name_is_rejected` |
| T-RP-80-024 | F80 の test の step を `--test rp02 --test rp19--test rp80 --test rp81` にする(`--test` が前の binary の名前にくっつく) | `--test` が前の名前にくっついた形 | 同上 | `Err` で `rp19--test` を名指す(specified、同上。cargo は `rp19--test` を 1 つの名前と読むので、`rp80` が並びから消えたまま緑になる) | 陰性 | `a_test_flag_glued_to_the_previous_name_is_rejected` |
| T-RP-80-025 | F80 の 3 の step を `... --test rp19 -- --exact rp19_python_cases_are_ported--exact x` の形にする(`--exact` が前の名前にくっつく) | `--exact` が前の名前にくっついた形 | 同上 | `Err` で `ported--exact` を名指す(specified、同上。3 の step の名前の検査 010 より先に、くっついた旗で落ちる) | 陰性 | `an_exact_glued_to_the_previous_name_is_rejected` |
| T-RP-80-026 | F80 に 4 入力を別々に足す。(d) `run: --test rp80`(`run:` の値の先頭の旗)。(a) `run:` の行ではなく、行頭が `--test rp80` で始まる行(インデントなし)。(b) `run:` の行の中の `-- --skip x`(行頭の `--` でなく、直前が空白)。(c) `run: cargo test -- --ignored` | 行頭の `--` と、直前が空白の旗 | 直前が空白でない旗だけを見る | 4 つとも検査 5 は通る(derived、式 `[^[:space:]]--(...)` は直前の 1 字が空白でない時だけ当たる。行頭は直前の字が無く、空白の後ろは `[^[:space:]]` に当たらない) | 境界、陽性 | `a_flag_at_the_start_of_a_line_or_after_a_space_is_not_glued` |
| T-RP-80-027 | F80 に 2 入力を別々に足す。(a) コメントの行 `# note: rp19--skip is not used`。(b) `- name: run rp80--test again` | `run:` の外の行。くっついた旗の綴りが在る | `run:` を含む行だけを見る | どちらも検査 5 は通る(specified、足す条件「`run:` の行」。コメントと名前の文は見ない。001 の語の検査はコメントも数える(006)が、この検査は行の種類を見る) | 境界、陽性 | `a_glued_flag_outside_a_run_line_is_not_checked` |
| T-RP-80-028 | `270a3bd` の `.github/workflows/test.yml`(41 行目の `run:`)を `git show 270a3bd:.github/workflows/test.yml` で取った実物 | 実在の例。`recognizer_to_warm--skip` | くっついた旗 0 | `Err` で `warm--skip` を名指す。件数は 1(pseudo：`git show 270a3bd:.github/workflows/test.yml \| grep -oE '[^[:space:]]--(test\|skip\|exact\|ignored\|include-ignored)'` が `m--skip` の 1 件。2026-10-21 に打った。022 の (b) が同じ式で 0 になることが陽性対照) | 陰性 | `the_recorded_workflow_with_a_glued_skip_is_rejected` |
| T-RP-80-029 | F80 に step を 1 つ足す。`run:` の値が `\|`、`>`、`\|-` で始まり、command は次の行に在る形(3 つ別々)。もう 1 つは `- name:` の次の行の `run: \|` | 複数行の `run:`。今の読み手は 1 行目しか読まず、旗の検査が届かない | 値が `\|` か `>` で始まる step 0 | どれも `Err` で `multi-line` と、足した `run:` の行番号(`line N:`)を名指す(specified、評議の条件 1。pseudo：`grep -c 'run: \|'` が今の `test.yml` で 0。落とす条件は値の先頭の字だけで、`run:` の行の外は読まない) | 境界、陰性 | `a_multi_line_run_is_rejected_and_its_line_is_named` |
| T-RP-80-030 | F80 の test の step の `--test rp80 --test rp81` の間を、(a) tab 1 字にした物。(b) U+00A0 にした物 | 空白の種類。shell が語を切るのは半角空白と tab | tab は空白、U+00A0 は空白でない | (a) は `Ok`。(b) は `Err` で `rp80` と U+00A0 と `--test` の続いた 1 語を名指す(specified、評議の条件 2。設計の「半角空白か tab か値の先頭」に揃え、検査 5 の式を `[^[:space:]]` にした。pseudo：`printf 'x\\t--skip y' \| grep -oE '[^[:space:]]--skip' \| wc -l` が 0、`printf 'x--skip y'` が 1、U+00A0 の前は 1。2026-10-21 に打った) | 境界、陽性、陰性 | `a_tab_before_a_flag_is_not_glued_but_a_no_break_space_is` |

**初めての実行**：018 は clone の最初の release の build で、target の cache が無い。sherpa-onnx の archive は build script が網から取る（D 10 の H103）。時間は NOT VERIFIED。
**始めてよい条件**：R12a1、R12a2、R11a1、R11a2、R11b が main に在り、評議の GO が state.json に在る（D 8 の「走らせる前の壊れ方」）。
**承認を待って止まる command**：なし。clone の dir は `~/.cache/hayamimi-r11/ci-probe` に決め打ち、無ければ作る。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- `windows-latest` と `ubuntu-latest` の matrix と `libasound2-dev` の step。D 8 の R11c の小節が書くが、RP-80 の期待値の列に無いので、行にしていない。
- 数の step が log に slice ごとの数を出すこと。GitHub 上の最初の run を見るまで機械では当たらない（D 10 の H103）。fixture の行は `continue-on-error` と command の綴りまでを見る。
- 016 は `--test rp19` の file が無い workflow を落とす行で、RP-80 の期待値の列に直接は無い。数の step が空振りして黙るのを防ぐ導出である。設計が不要と決めれば、この行を外す。

### 4.74 RP-81（文書と comment が、消えた Python の file を名指さない）

**未実装。** この節の行はどれも、test の file も関数も repo に無い。R11c の席が足す。
test：`native/hayamimi-eval/tests/rp81.rs`。受入 test の名前は `rp81_no_document_names_a_removed_python_file`。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp81`。`--ignored` は付けない。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない（014 と 030 は受入 test の名前の全部）。fixture は検査の道具の例外に合わせ、`native/hayamimi-eval/tests/rp81_*/` の dir に置く。
設計は D 3.9 と、D 6 と 7 の RP-81、8 の「R11 までの組」の R11a1、R11a2、R11b の行である。

`check_python_names(files, removed, owned)` の `files` は、`git ls-files` が出す file の対（path、text）の一覧である。tracked でない file は一覧に入らない。`removed` は `rp81_removed_python.txt` の 78 行、`owned` は `rp81_owned_lines.tsv` の行（file、行の文字列、持ち主の slice）である。
式は設計の `(scripts|demo)[/\\][a-z_0-9]+\.py|tests[/\\]test_[a-z_0-9]+\.py`（`/` と `\` の両方の区切りを読み、`demo/` を含む）と、dir を伴わない 78 本の file の名前（`download_models.py` の形。`removed` の各行の末尾の名前）を行ごとに当てる。当たった file が次のどれかなら通し、どれでもなければ `Err` でその file と行を名指す。

- **記録の dir**：path の最初の部分が `docs/eval/`、`docs/design/`、`docs/results/` のどれかである（dir の部分の単位で比べる）。
- **記録の file**：path が `mobile/hayamimi_core/CHANGELOG.md`、`tests/golden/ja/golden.json`、`mobile/hayamimi_core/test/fixtures/punct_ja_parity.json`、`docs/verify/android_emulator.md` のどれかとちょうど同じである。
- **検査の道具**：path が `native/hayamimi-eval/tests/rp81.rs`、`native/hayamimi-eval/tests/rp81_removed_python.txt`、`native/hayamimi-eval/tests/rp81_owned_lines.tsv` のどれかとちょうど同じか、`native/hayamimi-eval/tests/rp81_*/` の dir の下（T の fixture の木）である。
- **持ち主の在る例外（file、行の文字列）**：範囲の段落を付けて残す 6 file（`docs/spec/ja_pipeline.md`、`ja_pipeline.ja.md`、`docs/guide/embedding.md`、`embedding.ja.md`、`tuning.md`、`tuning.ja.md`）と `docs/spec/ja_pipeline_spec.json` の、当たる行。`owned` に（file、行の文字列）の対が在る行だけ通す。対に無い行が同じ file に増えるか、行の文字列が替わると `Err` でその file と行を名指す。
- **残す文字列の対（file、文字列）**：実行される文字列の 6 つ。
  1. `mobile/hayamimi_core/test/punct/punct_ja_fixture_tokens_test.dart`：`differently from scripts/punct_ja.py`
  2. `mobile/hayamimi_core/test/punct/punct_ja_parity_test.dart`：`PunctuatorJa matches scripts/punct_ja.py`
  3. 同じ file：`python scripts/quantize_punct.py --variant fp16`
  4. `mobile/hayamimi_core/test/punct/punct_worker_load_test.dart`：`python scripts/quantize_punct.py --variant fp16`
  5. と 6. `mobile/lib/main.dart`：画面の文の `scripts/eval_accuracy.py`（2 か所。2026-10-20 に 177 行目と 219 行目）

対の file では、当たった行がその file の対の文字列を含むときだけ通す。同じ file の別の行（comment）の当たりは通さない。

`check_removed_list(text, pinned_sha256, pinned_rows)` は、`rp81_removed_python.txt` の内容の sha256 と行の数が 2 つの定数（`REMOVED_PYTHON_SHA256`、`REMOVED_PYTHON_ROWS`。78）と同じであることと、全行が式の形に当たることを確かめる。`check_owned_lines(text, pinned_sha256, pinned_rows)` は `rp81_owned_lines.tsv` について `OWNED_LINES_SHA256`、`OWNED_LINES_ROWS` と同じであることを確かめる。`check_owned_exceptions_gone(text)` は tsv に行が 1 行でも在れば `Err` で持ち主ごとの数を言う。

**F81**（基準の fixture）：`README.md`（`cargo run` の command だけ）、`docs/guide/embedding.md`（`scripts/x.sh` を 1 行）、`native/hayamimi/src/lib.rs`、`mobile/lib/main.dart`（残す文字列の 2 つだけ）、`docs/design/rust_port.ja.md`（`scripts/eval_engine.py` を 1 行）の対の一覧。`removed` は `scripts/download_models.py`、`demo/capture_events.py`、`tests/test_units.py` の 3 行（定数は 3 と、その 3 行の sha256）、`owned` は空である。持ち主の在る例外の行の case（023 から 026）は、`docs/guide/tuning.md` に `scripts/eval_engine.py` を含む 1 行を持つ木と、`owned` に（`docs/guide/tuning.md`、その行、`R13-14`）の 1 行を持つ tsv（定数は 1 と、その行の sha256）を F81 に足して使う。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-81-001 | F81 そのまま | 正しい木。記録の dir の当たりと残す文字列だけが在る | 当たる file のうち例外でない物 0 | `Ok`（specified、D 6 の RP-81 の期待値）。以後の行は、この fixture から 1 か所だけ変える | 陽性 | `a_tree_without_a_removed_python_name_passes` |
| T-RP-81-002 | F81 の `docs/guide/embedding.md` に `scripts/x.py` を 1 行足す | 例外でない dir の当たり | 例外でない当たり 0 | `Err` で `docs/guide/embedding.md` を名指す（specified、D 7 の RP-81 の陰性の case「`docs/guide/` の md に `scripts/x.py` を 1 行足した木」） | 陰性 | `a_guide_that_names_a_removed_script_is_rejected_by_name` |
| T-RP-81-003 | R11 の前の `README.md`（`rp81_before_r11/README.md` に複製）を F81 の `README.md` と入れ替える | 検知の道具の陽性対照。式に当たる行が在る | 当たる行 16 | `Err` で `README.md` を名指し、当たる行を 16 本挙げる。pseudo：`rg -c -e '(scripts\|demo)[/\\][a-z_0-9]+\.py\|tests[/\\]test_[a-z_0-9]+\.py' README.md` が 16（実測 2026-10-20 12:0x、c83176b の木。`/` だけの式では 11。式を `scripts/zzz_nonexistent_zzz\.py` に替えると出力なし）。dir を伴わない 78 本の名前も足した式では 20 になる（同じ実測。この行は形の式の 16 を固定し、名前の 4 行は 021 が持つ）（specified、D 6 の RP-81 の陽性対照「R11 の前の木で `README.md`（16 件）」） | 陽性 | `the_readme_before_r11_has_sixteen_hits_and_is_rejected` |
| T-RP-81-004 | F81 の `docs/eval/a.md`、`docs/design/a.md`、`docs/results/a.md` のそれぞれに `scripts/x.py` を 1 行足した 3 つの木 | 記録の dir 3 つ。1 つずつ | 例外でない当たり 0 | 3 つとも `Ok`（specified、D 6 の RP-81 の期待値の最初の群） | 陽性 | `the_three_record_directories_may_name_a_python_file` |
| T-RP-81-005 | F81 に `docs/evaluation/a.md`、`docs/designs/a.md`、`docs/results2/a.md` のそれぞれへ `scripts/x.py` を 1 行足した 3 つの木 | 記録の dir と名前の前半だけが同じ dir。004 の隣 | dir の部分の単位 | 3 つとも `Err` でその file を名指す（derived、「`docs/eval/` の下」は dir の部分の一致で、文字列の前方一致ではない） | 境界、陰性 | `a_directory_that_only_shares_a_prefix_is_not_a_record_directory` |
| T-RP-81-006 | F81 の 4 つの記録の file のそれぞれに `scripts/x.py` を 1 行足した 4 つの木 | 記録の file 4 つ | path がちょうど同じ | 4 つとも `Ok`（specified、D 6 の RP-81 の期待値の記録の file） | 陽性 | `the_four_record_files_may_name_a_python_file` |
| T-RP-81-007 | F81 の `mobile/CHANGELOG.md` と、repo の根の `CHANGELOG.md` に `scripts/x.py` を 1 行足した 2 つの木 | 記録の file と名前だけが同じ別の path。006 の隣 | path がちょうど同じ | 2 つとも `Err` でその file を名指す（derived、記録の file は path で名指す） | 境界、陰性 | `a_changelog_at_another_path_is_not_a_record_file` |
| T-RP-81-008 | F81 の `mobile/lib/main.dart`、`mobile/hayamimi_core/test/punct/` の 3 file に、残す文字列の 6 つだけを置いた木 | 残す文字列の対 6 つ | 対の文字列を含む行 | `Ok`（specified、D 6 の RP-81 の期待値の「mobile の Dart の実行される文字列 6 つ」） | 陽性 | `the_six_kept_dart_strings_pass` |
| T-RP-81-009 | 008 の `punct_ja_parity_test.dart` に、残す文字列を含まない comment の行（`///` に続けて `scripts/punct_ja.py` と書く行）を足す | 対の file の中の、対でない当たり。実物の同じ file は 2026-10-20 に 4 件（comment 2 + 残す 2）で、R11b の後は 2 件 | 対の文字列を含む行だけ | `Err` で `punct_ja_parity_test.dart` と足した行を名指す（specified、D 6 の RP-81「file と残す文字列の対の表で許す」。file ごと許すと comment が残る） | 境界、陰性 | `an_extra_hit_in_a_file_with_a_kept_string_is_rejected` |
| T-RP-81-010 | 008 の `PunctuatorJa matches scripts/punct_ja.py` を `mobile/hayamimi_core/test/punct/punct_ja_text_test.dart` に移した木 | 残す文字列が別の file に在る | 対の file | `Err` でその file を名指す（specified、同上。対は file と文字列の組） | 境界、陰性 | `a_kept_string_in_another_file_is_rejected` |
| T-RP-81-011 | F81 に `scripts/x.sh` と `tests/test_a.rs` を含む 1 行ずつを足した木 | 式に似て当たらない名前（拡張子が `.py` でない） | 例外でない当たり 0 | `Ok`（derived、式は `.py` まで要る。検知の道具が似た名前に当たらない陰性対照） | 境界、陽性 | `names_that_only_resemble_a_removed_python_file_pass` |
| T-RP-81-012 | F81 の `README.md` に `scripts/a1_b2.py` の行と、`tests/test_a.py` の行を別々に足した 2 つの木 | 式の 2 つの選択肢。数字と下線を含む名前 | 例外でない当たり 0 | 2 つとも `Err` で `README.md` を名指す（specified、D 7 の RP-81 の式。`[a-z_0-9]+` が数字と下線を含む） | 境界、陰性 | `both_removed_path_forms_are_matched` |
| T-RP-81-013 | F81 の `native/hayamimi/src/lib.rs` に `// ported from scripts/asr_engine.py` の comment を足す | source の comment。文書でない consumer | 例外でない当たり 0 | `Err` で `lib.rs` を名指す（specified、D 6 の RP-81 の題「文書と comment」。実物では `native/hayamimi-relay/tests/support/events.rs` の 6 行目の comment が 2026-10-20 に当たる（R11c が直す）） | 陰性 | `a_source_comment_naming_a_removed_script_is_rejected` |
| T-RP-81-014 | 受入：`git ls-files` の実物の木（R11a1、R11a2、R11b、R11c の後） | 全体。001 から 013 と 015 から 029 の判定を実物に掛ける | 例外でない当たり 0 | `Ok`。pseudo：`git ls-files -z \| xargs -0 grep -lE '(scripts\|demo)[/\\][a-z_0-9]+\.py\|tests[/\\]test_[a-z_0-9]+\.py'` の出力の各 file が、記録の dir、記録の file、検査の道具（`rp81.rs`、`rp81_removed_python.txt`、`rp81_owned_lines.tsv`、fixture の木 `rp81_*/`）、持ち主の在る例外の file、残す文字列の対の file のどれかである（R11 の前の木では 145 file で、`.py` の外は 79 file。実測 2026-10-20 12:0x。`rg -l` でも 145） | 陽性、陰性 | `rp81_no_document_names_a_removed_python_file` |
| T-RP-81-015 | 3 行の `removed` と、その sha256 と 3 を定数に持つ fixture | 一覧が 2 つの定数と合う | 定数 2 つ | `Ok`（specified、D 6 の RP-81「一覧が 2 つの定数と合う」） | 陽性 | `the_removed_python_list_matches_its_two_pins` |
| T-RP-81-016 | 015 の `removed` に `scripts/extra.py` の 1 行を足した物（定数は 3 のまま） | 一覧に行が増えた | 行の数 3 | `Err` で行の数の食い違い（4 と 3）と sha256 の食い違いを言う（specified、同上。rp19 の固定の一覧と同じ形） | 陰性 | `a_removed_python_list_that_drifted_from_its_pins_is_rejected` |
| T-RP-81-017 | 015 の 3 行（`scripts/download_models.py`、`demo/capture_events.py`、`tests/test_units.py`）。実物は `rp81_removed_python.txt` の 78 行 | 全行が式の形に当たる | 全行 | 3 行とも式に当たって `Ok`。実物は 78 行が全部当たる（specified、D 6 の RP-81。pseudo：`git ls-files '*.py' \| rg -c -e '(scripts\|demo)[/\\][a-z_0-9]+\.py\|tests[/\\]test_[a-z_0-9]+\.py'` が 78、`git ls-files '*.py' \| wc -l` も 78。実測 2026-10-20 12:0x。式の外の `tools/x.py` では出力なし） | 陽性 | `every_line_of_the_removed_python_list_is_matched_by_the_pattern` |
| T-RP-81-018 | 015 の `removed` に式に当たらない path `tools/x.py` を 1 行足した物（定数もそれに合わせる） | 一覧の行が式を外れる。定数は合っている | 全行 | `Err` で `tools/x.py` を名指す（specified、D 7 の RP-81 の陰性の case「式に当たらない path を 1 行足した木」。式が消した file を名指せなくなる変更を止める） | 陰性 | `a_removed_list_line_the_pattern_misses_is_rejected` |
| T-RP-81-019 | F81 の `README.md` に `scripts\x.py`、`.venv\Scripts\python scripts\realtime_transcribe.py`、`tests\test_a.py` を別々に足した 3 つの木 | `\` の区切り。`/` の 001 から 013 の対 | 区切り 2 種 | 3 つとも `Err` で `README.md` を名指す（specified、D 7 の RP-81 の陰性の case「`scripts\x.py` を 1 行足した木」。Windows の command を当てる） | 境界、陰性 | `a_backslash_path_to_a_removed_script_is_rejected` |
| T-RP-81-020 | F81 の `README.md` に `demo/x.py`、`demo\x.py`、`demo/capture_events.py` を別々に足した 3 つの木 | `demo/` の dir。2 種の区切りと、消す file の実名 | dir 3 種（`scripts`、`demo`、`tests`） | 3 つとも `Err` で `README.md` を名指す（specified、D 7 の RP-81 の陰性の case「`demo/x.py` を足した木」） | 境界、陰性 | `a_demo_script_path_is_rejected` |
| T-RP-81-021 | F81 の `README.md` に `python download_models.py` の行を足した木 | dir を伴わない消した file の名前。形の式には当たらない | `removed` の名前 | `Err` で `README.md` と `download_models.py` を名指す（specified、D 7 の RP-81 の陰性の case「dir の無い `download_models.py` を足した木」。実物の `README.md` は名前を足した式で 20 行、形の式で 16 行で、差の 4 行がこの形） | 陰性 | `a_bare_removed_file_name_is_rejected` |
| T-RP-81-022 | F81 の `README.md` に `python my_tool.py` の行を足した木 | `removed` に無い名前。021 の隣 | `removed` の名前だけ | `Ok`（derived、名前の検査は 78 本の一覧の名前だけを当てる。検知の道具が `.py` の名前すべてに当たらない陰性対照） | 境界、陽性 | `a_bare_python_name_outside_the_removed_list_passes` |
| T-RP-81-023 | F81 に足した `docs/guide/tuning.md` に、`owned` に無い当たりの行（`scripts/asr_engine.py` を含む 1 行）を足した木 | 持ち主の在る例外の file の、対に無い行 | 対に在る行だけ | `Err` で `docs/guide/tuning.md` と足した行を名指す（specified、D 7 の RP-81 の陰性の case「範囲の段落の file に tsv に無い当たりの行を 1 行足した木」。消し忘れと書き足しを隠さない） | 陰性 | `an_unlisted_line_in_an_owned_file_is_rejected` |
| T-RP-81-024 | F81 の `owned` に在る行だけを持つ、6 file と `docs/spec/ja_pipeline_spec.json` の 7 つの木 | 持ち主の在る例外の file。対に在る行 | 対に在る行 | 7 つとも `Ok`（specified、D 6 の RP-81 の期待値の 3 つ目の種類） | 陽性 | `the_listed_lines_of_an_owned_file_pass` |
| T-RP-81-025 | 024 の `docs/guide/tuning.md` の行の文字列を 1 字変えた木（`owned` は変えない） | 対の file は同じで、行の文字列が替わった。023 の隣 | 行の文字列 | `Err` で file と替わった行を名指す（specified、D 6 の RP-81「行の文字列が替わると落ちる」） | 境界、陰性 | `a_listed_line_whose_text_changed_is_rejected` |
| T-RP-81-026 | F81 の `docs/spec/ja_pipeline_spec.json` に `owned` に無い当たりの行を足した木 | 記録の file から持ち主の在る例外に移った file | 対に在る行だけ | `Err` でその file を名指す（specified、D 6 の RP-81 の記録の file の一覧に `ja_pipeline_spec.json` が無く、持ち主は R13-10） | 境界、陰性 | `the_spec_json_is_owned_and_no_longer_a_record_file` |
| T-RP-81-027 | 024 の `owned` の tsv に 1 行足した物（定数は 1 のまま） | tsv が定数から外れた | 行の数 1 | `Err` で行の数の食い違い（2 と 1）と sha256 の食い違いを言う（specified、D 6 の RP-81「sha256 と行の数を `OWNED_LINES_SHA256` と `OWNED_LINES_ROWS` に置く」） | 陰性 | `an_owned_lines_file_that_drifted_from_its_pins_is_rejected` |
| T-RP-81-028 | F81 の `native/hayamimi-eval/tests/rp81.rs`、`rp81_removed_python.txt`、`rp81_owned_lines.tsv`、`native/hayamimi-eval/tests/rp81_a/x.md` のそれぞれに `scripts/x.py` を 1 行足した 4 つの木 | 検査の道具 3 つと fixture の木 | path がちょうど同じか dir の下 | 4 つとも `Ok`（specified、D 6 の RP-81 の期待値の 2 つ目の種類） | 陽性 | `the_checking_tool_files_may_name_a_python_file` |
| T-RP-81-029 | F81 の `native/hayamimi-eval/tests/rp80.rs`、`native/hayamimi-eval/tests/rp81_a.rs`、`native/hayamimi/tests/rp81_a/x.md`、`native/hayamimi-eval/tests/rp81a/x.md` のそれぞれに `scripts/x.py` を 1 行足した 4 つの木 | 検査の道具に似た path。028 の隣 | path がちょうど同じか `rp81_*/` の dir の下 | 4 つとも `Err` でその file を名指す（derived、道具の例外は path と dir で名指す。名前の前半が同じだけの file と、別の crate の dir は通さない） | 境界、陰性 | `a_sibling_of_the_tool_files_is_not_exempt` |
| T-RP-81-030 | 完了：repo の `native/hayamimi-eval/tests/rp81_owned_lines.tsv` の実物 | tsv の行が 0 | 行 0 | tsv に行が 0 で `Ok`。R11c の時点では行が在るので落ち、持ち主ごとの数を出す（specified、D 7 の RP-81（完了）の行。R13-10 と R13-14 が行を消した後に通る）。pseudo：`wc -l < native/hayamimi-eval/tests/rp81_owned_lines.tsv` が 0 | 陽性、陰性 | `rp81_owned_exceptions_are_gone` |
| T-RP-81-031 | tsv の fixture：`R13-10` の行 1 つと `R13-14` の行 2 つ | 検知の道具の陽性対照。行が残る | 行 1 以上 | `Err` で `R13-10` が 1、`R13-14` が 2 と持ち主ごとの数を言う（specified、D 7 の RP-81（完了）の「持ち主ごとの数を出す」） | 陰性 | `a_remaining_owned_row_is_counted_per_owner` |
| T-RP-81-032 | 空の tsv（0 byte） | 行が 0 の境界。031 の隣 | 行 0 | `Ok`（specified、同上。行が無ければ通る。027 の定数は 0 と空の内容の sha256） | 境界、陽性 | `an_empty_owned_lines_file_passes` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 大文字や `-` を含む名前（`scripts/Foo.py`、`scripts/a-b.py`）は式に当たらない。消す 78 本にそのような名前は無いので、行にしていない。
- 消した名前を末尾に含む別の名前（`my_download_models.py`）が、dir を伴わない名前の検査に当たるかは、設計に書いていないので行にしていない。
- 文字コードが UTF-8 でない tracked file の読み方。`git ls-files` の 145 file は `grep -lE` と `rg -l` が同じ数で、今の木に binary の当たりは無い。

### 4.75 RP-83（E2E の run の門）

**001 から 054 は `native/hayamimi-eval/tests/rp83.rs` の 54 本と 1 対 1 で実装済み**（R8x-1）。**055 と、足した 056 から 090（run 自身の分、`--own-pid`、`own` を持たない古い形）は、実装が追いついていない。** R8x-9 の席が `rp83.rs` と `tests/support/machine.rs` と `tests/support/s83.rs` を直すまで通らない。001 から 054 の関数の末尾は変えない（実装との 1 対 1 を保つため）。S83o の起動の引数に `--own-llama-port 18084` を足す直し（`tests/support/s83.rs` の `own_args` と、`rp83.rs` の `--own-pid` を渡す呼び）は、IP-8 を実装する IP-S1 の席が同じ commit で行う（設計 10.11 の IP-S1 と 10.12 の H41）。
test：`native/hayamimi-eval/tests/rp83.rs`。受入 test の名前は `rp83_a_run_under_a_busy_machine_or_beside_a_build_is_refused`（実装済み。main の `rp83.rs` に在る）。実走の行は無く、全部の行が fixture を読み、`--ignored` を付けずに通る。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp83`。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない（041 だけは受入 test の名前の全部）。判定の関数の名前と引数の形は実装の席が決める。行は「run の列（名前、Rust か Python か、`<run>.machine.jsonl` の text か無し）を渡す」形で書く。
設計は D 3.19 の「E2E の run の門」と、D 6 と 7 の RP-83、D 10 の H118 と H124 である。

**読み方**：

- 「行 i」は `<run>.machine.jsonl` の i 行目（1 始まり）。「最初の行」は行 1、「最後の行」は最後の行。
- 負荷の値（`load1`、`load5`、`load15`）は、`/proc/loadavg` の text の 2 桁の小数を **JSON の文字列** で持つ（`"2.00"`）。10 の -2 乗の整数に読んで比べる（`"2.00"` は 200）。JSON の数は、文字列でない値として読めない値に入り、落ちる（D 3.19 の「E2E の run の門」の 4）。`t_us`、`cargo`、`rustc` は JSON の整数。
- 以下の行の「`load15` を `2.50` にする」のような書き方は、file の中では `"load15":"2.50"` の文字列の意味である。
- run の列の 1 つは（dir、名前、Rust か Python か、`<run>.machine.jsonl` の text か無し）で、判定は dir の `<名前>.jsonl` を読んで sha256 を取る（門の前の記録の一覧に当たるかを見るため）。fixture の run の dir は `target/` の下の一時 dir で、`<名前>.jsonl` は任意の 1 行の text でよい。一覧に当たるかを見る行（045 から 048、051、052）だけが、repo の `docs/results/` の実物の `<名前>.jsonl` を読む。
- 「名指す」は、`Err` の text が run の名前、欄の名前、値の文字列（`2.50` の形）の全部を含むこと。
- `stream` の行（030 から 040、043、044）は、実際の `/proc` を読ませない。test 自身が `cargo` の下で走り、実際の `/proc` には `cargo` の process が必ず在って、どの行も拒まれるからである。`--proc-root` に fixture の dir を渡す。
- 欄の名前（`load1`、`load5`、`load15`、`cargo`、`rustc`、`t_us`）は設計が挙げる。`stream.machine.refused` の `reason` と値を載せる欄の綴りは設計に無いので、test はその綴りを 1 か所の定数に持つ（4.1）。

**F83**（式で作る。実測のふりをしない）：Rust の run `rs1` の `rs1.machine.jsonl`。5 行で、`t_us` は 0、5000000、10000000、15000000、20000000。全行が `load1` `"0.50"`、`load5` `"0.50"`、`load15` `"0.50"`（JSON の文字列）、`cargo` 0、`rustc` 0。

**S83**（`stream` を起こす行の組）：

- 子の process は `env!("CARGO_BIN_EXE_hayamimi-eval")` を `stream --host 127.0.0.1 --port <偽の /ingest> --wav <wav> --manifest <manifest> --repeat 1 --gap-ms 0 --drain-s 1 --out <dir>/s83.jsonl --proc-root <proc-root>` で起こす。`<dir>` と `<proc-root>` は `target/` の下の一時 dir（`/tmp` は使わない）。
- `<proc-root>` は `loadavg`（`0.50 0.50 0.50 1/300 1000` の 1 行。行ごとに書き換える）を持ち、既定では pid の dir を持たない。pid の dir は `<proc-root>/<pid>/comm`（中身は名前と改行 1 つ）の形で足す。
- wav は 16 kHz の無音。長さは 030 から 038 が 1 秒、039 と 040 が 12 秒。manifest は 1 発話（開始 0、終わりは wav の長さ）。
- 偽の `/ingest` は test 自身の listener（port 0。OS が割り当てる）で、ws の handshake を済ませ、binary frame の payload の byte を数える。固定の port を取らない。
- 040 だけは `--drain-s 11` にする（drain の間の行を見るため）。ほかの行は `--drain-s 1`。
- 資源と時間：GPU、DB、他の port は使わない。`stream` は実時間で送るので、040 が約 24 秒（12 秒 + 11 秒 + 起動）、039 が約 13 秒、ほかは 3 秒以内（wav の長さ + `--drain-s` 1 秒）。

**O83**（`own` を持つ machine の file。式で作る。056 から 066 の判定の行）：Rust の run `rs1` の `rs1.machine.jsonl`。106 行で、`t_us` は 0 から 5000000 ずつ増えて 525000000 まで。行 1 の `load1` は `"1.72"`、`load15` は `"2.13"`。最後の行（行 106）の `load5` は `"3.10"`。ほかの load の欄は `"0.50"`、`cargo` と `rustc` は全行 0。行 1 は `own`（`{"pid": 整数, "comm": 文字列}` の配列。4242 `hayamimi`、4243 `hayamimi-relay`、4244 `llama-server` の 3 つ）と `clk_tck` 100 を持つ。全行が `own_cpu_ticks`（JSON の整数。行 1 は 0、以後 1 行ごとに c ずつ増える）と `cpu_some_total`（JSON の `null`）を持つ。c は行の中で決める。
`own5` と判定の値の式（pseudo。2026-10-21 に打った）：`awk -v c=<c> 'BEGIN{o=0;a=exp(-5/300);for(k=0;k<105;k++)o=o*a+c/500*(1-a);printf "%.6f %d\n",o,int(o*100)}'`。105 は行 1 の後の行の数、500 は `clk_tck` 100 × 行の間 5 秒である。出力の 2 つ目が `own5` の 100 倍の切り捨てで、判定は 310 から引き、250 未満で通す。

| c | `own5` | 100 倍の切り捨て | 310 − 切り捨て | 判定 |
|---|---|---|---|---|
| 364 | 0.601493 | 60 | 250 | 落ちる |
| 369 | 0.609755 | 60 | 250 | 落ちる |
| 370 | 0.611407 | 61 | 249 | 通る |

**2 行版**（059、060）：`t_us` が 0 と 15000000 の 2 行。行 1 の `load1` `"1.00"`、`load15` `"1.00"`、`own_cpu_ticks` 0。行 2 の `own_cpu_ticks` は 15000（`clk_tck` 100 で 15 秒に 10 core 分）。`own5` は `awk 'BEGIN{a=exp(-15/300);o=15000/(100*15)*(1-a);printf "%.6f %d\n",o,int(o*100)}'` で 0.487706、切り捨てが 48（2026-10-21 に打った）。判定は行 2 の `load5` の 100 倍から 48 を引く。

**S83o**（`stream` を `--own-pid` つきで起こす行の組）：S83 に、`<proc-root>` の `4242/comm` が `hayamimi`、`4243/comm` が `hayamimi-relay`、`4244/comm` が `llama-server` を足す（どれも改行つき）。`4244/cmdline` は NUL 区切りの `llama-server`、`-m`、`model.gguf`、`--port`、`18084` の 5 語に、最後の語の後ろにも NUL を置いた物（`/proc/<pid>/cmdline` の書式。proc_pid_cmdline(5)）。`4242` と `4243` は `cmdline` の file を持たない（comm が `llama-server` の pid だけが `cmdline` を読まれる）。3 つの pid に `stat` の file を置く（`4242 (hayamimi) S 1 4242 4242 0 -1 4194560 100 0 0 0 1000 250 0 0 20 0 8 0 12345 1000000 100 18446744073709551615` の形。14 番目と 15 番目が utime と stime）。utime と stime は、4242 が 1000 と 250、4243 が 600 と 100、4244 が 1500 と 301 で、和の合計は 3751 である（1250 + 700 + 1801。どの 1 つの pid の値の 3 倍（3750、2100、5403）とも等しくない）。起こす引数に `--own-pid 4242 --own-pid 4243 --own-pid 4244 --own-llama-port 18084` を足す（`--own-llama-port` は `--own-pid` と互いに要り合い、既定の値を持たないので、`--own-pid` を渡す `stream` の起動は必ず両方を持つ。値は `4244/cmdline` の `--port` の次の語と同じ 18084。設計 `conversation_subtitles.ja.md` 10.7.1）。足す pid（4245 以降）には、各行が言う comm の file と、同じ形の `stat` の file（utime と stime は 10 と 10 でよい）を置く。comm が受けられない pid（`sudo`、`bash`、`hayamimi-foo`）にも、`--port` が違う `llama-server` の pid にも置く。確かめは pid ごとに `unreadable`、`comm`、`port` の順に打たれ、`stat` が無い pid は `comm` より先に `unreadable` で拒まれるからである。`stat` を置かないのは、読めない pid の行（076）だけである。`stream` 自身の pid は `--own-pid` に足されないので、`<proc-root>` に置く必要が無い。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-83-001 | F83 の `rs1` そのまま | 全欄が基準の内側 | `Ok` | `Ok`（specified、RP-83 の (1)） | 陽性 | `a_clean_run_passes` |
| T-RP-83-002 | 001 で行 1 の `load1` を `1.99` にする | `load1` の線の 1 つ手前 | `load1` < 2.00 | `Ok`（specified、RP-83 の (1)。199 < 200） | 境界、陽性 | `a_first_row_load1_of_1_99_passes` |
| T-RP-83-003 | 001 で行 1 の `load1` を `2.00` にする | `load1` の線の値 | 同上 | `Err` で `rs1`、`load1`、`2.00` を名指す（specified、RP-83 の (1)。200 は 2.00 未満でない。T-RP-32-009 と同じ境界） | 境界、陰性 | `a_first_row_load1_of_2_00_is_rejected` |
| T-RP-83-004 | 001 で行 1 の `load15` を `2.49` にする | `load15` の線の 1 つ手前 | `load15` < 2.50 | `Ok`（specified、RP-83 の (1)。249 < 250） | 境界、陽性 | `a_first_row_load15_of_2_49_passes` |
| T-RP-83-005 | 001 で行 1 の `load15` を `2.50` にする | `load15` の線の値 | 同上 | `Err` で `rs1`、`load15`、`2.50` を名指す（specified、同上。前の 15 分平均） | 境界、陰性 | `a_first_row_load15_of_2_50_is_rejected` |
| T-RP-83-006 | 001 で最後の行（行 5）の `load5` を `2.49` にする | 後の `load5` の線の 1 つ手前 | `load5` < 2.50 | `Ok`（specified、RP-83 の (1)。249 < 250） | 境界、陽性 | `a_last_row_load5_of_2_49_passes` |
| T-RP-83-007 | 001 で最後の行の `load5` を `2.50` にする | 後の `load5` の線の値 | 同上 | `Err` で `rs1`、`load5`、`2.50` を名指す（specified、同上。後の 5 分平均） | 境界、陰性 | `a_last_row_load5_of_2_50_is_rejected` |
| T-RP-83-008 | 001 で、行 1 の `load5` を `9.99`、行 5 の `load1` と `load15` を `9.99`、行 2 から行 4 の `load1`、`load5`、`load15` を `9.99` にする | 判定に入らない欄 | 判定に入らない | `Ok`（specified、RP-83 の (1) が挙げる欄は、行 1 の `load1` と `load15`、最後の行の `load5` だけ。後の 1 分は run 自身が押し上げる値で、3.19 は判定に使わない。T-RP-32-014 の後の 1 分と同じ向き） | 境界 | `loads_the_gate_does_not_judge_may_be_high` |
| T-RP-83-009 | 001 で行 3 の `cargo` を `1` にする（ほかの行は 0） | run の途中の build（1 行だけ） | `cargo` が全行で 0 | `Err` で `rs1`、`cargo`、`1` を名指す（specified、RP-83 の (1) の「全行の `cargo` と `rustc` が 0」と、設計の陰性の case「途中の 1 行だけ `cargo` が 1」。前の 1 分の load では見えない形） | 陰性 | `a_cargo_in_one_middle_row_is_rejected` |
| T-RP-83-010 | 001 で行 3 の `rustc` を `1` にする | `rustc` の側 | `rustc` が全行で 0 | `Err` で `rs1`、`rustc`、`1` を名指す（specified、同上） | 陰性 | `a_rustc_in_one_middle_row_is_rejected` |
| T-RP-83-011 | 001 で行 1 の `cargo` を `1` にする | 最初の行も全行に入る | 同上 | `Err` で `rs1`、`cargo`、`1` を名指す（specified、同上。(2) の拒みと同じ条件を (1) も持つ） | 境界、陰性 | `a_cargo_in_the_first_row_is_rejected` |
| T-RP-83-012 | 001 で最後の行の `rustc` を `1` にする | 最後の行も全行に入る | 同上 | `Err` で `rs1`、`rustc`、`1` を名指す（specified、同上） | 境界、陰性 | `a_rustc_in_the_last_row_is_rejected` |
| T-RP-83-013 | 001 で行 3 の `cargo` を `2` にする | 0 以外の別の値 | 同上 | `Err` で `cargo` と `2` を名指す（derived、条件は 0 であること。1 だけを落とす判定にしない） | 境界、陰性 | `two_cargo_processes_are_rejected_as_well` |
| T-RP-83-014 | 001 で `t_us` を 0、5000000、10000000、25000000、30000000 にする（行 3 と行 4 の間が 15000000） | 行の間の線ちょうど | 15 秒以下 | `Ok`（specified、RP-83 の (1)「行の間が 15 秒以下」。等号側） | 境界、陽性 | `a_gap_of_exactly_15_seconds_passes` |
| T-RP-83-015 | 001 で `t_us` を 0、5000000、10000000、25000001、30000001 にする（行 3 と行 4 の間が 15000001） | 線の 1 µs 上 | 同上 | `Err` で `rs1` と間の値 `15000001` を名指す（specified、同上。最小の桁 1 つ分だけ外れた値） | 境界、陰性 | `a_gap_of_15_seconds_and_one_microsecond_is_rejected` |
| T-RP-83-016 | 001 で `t_us` を 0、5000000、10000000、30000000、35000000 にする（行 3 と行 4 の間が 20 秒） | 大きい隙間（設計の陰性の case） | 同上 | `Err` で `rs1` と間の値 `20000000` を名指す（specified、RP-83 の陰性の case「行の間が 20 秒の fixture」） | 陰性 | `a_gap_of_20_seconds_is_rejected` |
| T-RP-83-017 | 001 の 2 入力。(a) `t_us` を 0、15000001、20000001、25000001、30000001 にする（行 1 と行 2 の間が 15000001）。(b) `t_us` を 0、5000000、10000000、15000000、30000001 にする（行 4 と行 5 の間が 15000001） | 隙間が最初の組と最後の組に在る | 15 秒以下 | どちらも `Err` で `15000001` を名指す（specified、RP-83 の (1)。「行の間」は隣り合う全部の組。隙間を行 2 から行 4 の間だけで見る判定は (a)(b) で落ちる） | 境界、陰性 | `a_gap_at_either_end_of_the_file_is_rejected` |
| T-RP-83-018 | 001 で `t_us` を 0、5000000、10000000、10000000、15000000 にする（行 3 と行 4 の間が 0） | 行の間の下側の境界 | 15 秒以下 | `Ok`（derived、RP-83 の (1)「行の間が 15 秒以下」。間が 0 の組も 15 秒以下に入る） | 境界、陽性 | `two_rows_at_the_same_time_pass` |
| T-RP-83-019 | 2 run の列：`py1`（Python。file 無し）と `rs1`（F83） | Python の基準の run | 掛けない | `Ok`（specified、RP-83 の (1)「Python の基準の run には掛けない」。`py1` は流し直さないので file を持たない） | 陽性 | `a_python_run_without_a_machine_file_is_not_judged` |
| T-RP-83-020 | 1 run の列：`rs1`（Rust。file 無し。dir は一時 dir で、`rs1.jsonl` の sha256 は門の前の記録の一覧に無い） | Rust の run の file の欠け。一覧に当たらない run | file が在る | `Err` で `rs1` と file が無いことを名指す（specified、RP-83 の (1)「file の無い run」と 3.19 の「門の前の記録」の「一覧に無い run は、file が無ければ落ちる」。確かめられないので通さない側に倒す） | 陰性 | `a_rust_run_without_a_machine_file_is_rejected` |
| T-RP-83-021 | `py1`（Python）に、行 1 の `load15` が `3.14` の file を付けた物 | Python の run は file が在っても掛けない | 掛けない | `Ok`（specified、同上。判定は run の実装で決まり、file の有無では決まらない） | 境界 | `a_python_run_is_not_judged_even_when_it_has_a_file` |
| T-RP-83-022 | 3 run の列：`rs1`（F83）、`rs2`（001 で行 1 の `load15` を `2.50`）、`rs3`（F83） | 1 本だけが外れる | run ごと | `Err` で `rs2`、`load15`、`2.50` を名指す。`rs1` と `rs3` は名指さない（specified、RP-83 の (1)「run の名前と外れた値を挙げて」） | 陰性 | `only_the_failing_run_is_named` |
| T-RP-83-023 | `docs/results/rp32/` の Rust の 3 run を写した 3 run の列。行 1 と行 5 の `load1 load5 load15` は `rs1` が `1.36 1.99 2.02` と `1.27 1.80 1.84`、`rs2` が `0.69 1.51 1.73` と `1.81 1.66 1.64`、`rs3` が `1.17 1.48 1.58` と `2.05 2.18 1.97`。行 2 から行 4 は行 1 と同じ値。`cargo` と `rustc` は 0 | 実物の記録の値（前の 15 分平均 1.58〜2.02、後の 5 分 1.66〜2.18。`rs3` の後の 1 分 2.05 は 2.00 以上） | 線の内側 | `Ok`（specified、RP-83 の陽性の case。pseudo：`awk '$1 ~ /^rs/ && $2=="before"{print $1,$4,$5,$6} $1 ~ /^rs/ && $2=="after"{print $1,$4,$5,$6}' docs/results/rp32/load1.txt docs/results/rp32/load2.txt docs/results/rp32/load3.txt` が上の 6 行。2026-10-20 に打った。後の 1 分が 2.05 の `rs3` が通ることで、後の 1 分を判定に使わないことも固定する） | 陽性 | `the_recorded_rp32_rust_loads_pass` |
| T-RP-83-024 | e-card-form の 3 run を写した 3 run の列。行 1 と行 5 の `load1 load5 load15` は `rs1` が `1.77 2.48 3.14` と `3.01 3.45 3.49`、`rs2` が `1.78 2.53 3.03` と `3.40 3.17 3.15`、`rs3` が `1.89 2.65 2.95` と `3.28 3.59 3.48`。ほかは 023 と同じ形 | 実物の記録の値。前の 1 分は 1.77〜1.89 で 2.00 未満（RP-32 の線を通る）。前の 15 分平均 2.95〜3.14、後の 5 分 3.17〜3.59 | 線の外側 | `Err` で `rs1`、`rs2`、`rs3` と、`load15` の `3.14`、`3.03`、`2.95` を名指す。後の `load5` の値は名指しても名指さなくてもよい（specified、RP-83 の陰性の case。pseudo：`awk '$2=="before"{print $1,$4,$5,$6} $2=="after"{print $1,$4,$5,$6}' load1.rs.txt load2.rs.txt load3.rs.txt` を e-card-form の dir で打った 6 行。2026-10-20 に打った） | 陰性 | `the_recorded_ecard_form_loads_are_rejected_at_load15` |
| T-RP-83-025 | 空の file（0 byte）の `rs1` | 行が 0 | 1 行以上 | `Err` で `rs1` と行が無いことを名指す（implicit、0 行を「外れが無い」と読んで通さない） | 境界、陰性 | `an_empty_machine_file_is_rejected` |
| T-RP-83-026 | 001 の最後の行を途中で切った file（`{"t_us":20000000,"load1":"0.5` で終わる） | 書きかけの最後の行 | 全行が読める | `Err` で `rs1` と読めない行を名指す（implicit、読めた 4 行だけで通さない。最後の行は `load5` の判定に使う行なので、欠けると判定できない） | 陰性 | `a_machine_file_cut_in_its_last_row_is_rejected` |
| T-RP-83-027 | 001 で行 3 から `rustc` の欄を除く | 欄の欠け | 全欄が在る | `Err` で `rs1` と `rustc` を名指す（implicit、欠けた欄を 0 と読んで通さない。T-RP-32-013 と同じ向き） | 陰性 | `a_row_without_a_rustc_field_is_rejected` |
| T-RP-83-028 | 001 で行 1 の `load15` を JSON の数 `0.50`（引用符なし）にする。値は線の内側 | 文字列でない load（値は読めるが型が違う） | 3 つの load は JSON の文字列 | `Err` で `rs1` と `load15` を名指す（specified、RP-83 の (4)「load の 3 つは JSON の文字列で、文字列でない値は読めない値として落ちる」。数を読めるからと通す判定は、ここで落ちる。001 の文字列 `"0.50"` が通ることが陽性対照） | 境界、陰性 | `a_load_written_as_a_json_number_is_rejected` |
| T-RP-83-029 | 2 行だけの file：`t_us` が 0 と 5000000、全欄が 0.50 と 0 | 判定できる最小の行数 | 線の内側 | `Ok`（derived、最初の行と最後の行が別の行で、全部の条件を読める最小の形） | 境界、陽性 | `a_machine_file_of_two_rows_passes` |
| T-RP-83-030 | S83。`loadavg` を `2.00 0.50 0.50 1/300 1000` にして `stream` を起こす（pid の dir は無い） | 最初の行の `load1` の線の値 | `load1` < 2.00 | exit code 2。stderr に `stream.machine.refused` が 1 件（level `warn`）で、`reason` が `load1`、値が `2.00`。偽の `/ingest` が受けた音声の byte は 0。`s83.machine.jsonl` は 1 行で `load1` が 2.00（specified、RP-83 の (2)。1 行の file は「音声を送り始める前に 1 行」を書く、3.19 の「E2E の run の門」の 1 から導く） | 境界、陰性 | `a_first_row_load1_of_2_00_stops_the_stream_before_any_audio` |
| T-RP-83-031 | S83。`loadavg` を `1.99 9.99 2.49 1/300 1000` にする | 3 つの線の 1 つ手前。判定に入らない `load5` は高い | 3 欄とも内側 | 始まる。exit code が 2 でも 3 でもない、`stream.machine.refused` と `stream.machine.aborted` が 0 件、偽の `/ingest` が受けた音声の byte が 1 以上。`s83.machine.jsonl` の行 1 は `load1` 1.99、`load15` 2.49（specified、RP-83 の (2)。拒む理由は `load1`、`load15`、build の 3 つだけ。exit code は音声が無音で `final` が来ないので 1 などになりうるが、2 と 3 ではない） | 境界、陽性 | `a_first_row_one_step_inside_every_line_starts_the_stream` |
| T-RP-83-032 | S83。`loadavg` を `0.50 0.50 2.50 1/300 1000` にする | 最初の行の `load15` の線の値 | `load15` < 2.50 | exit code 2。`stream.machine.refused` が 1 件、`reason` が `load15`、値が `2.50`。音声の byte は 0（specified、RP-83 の (2)） | 境界、陰性 | `a_first_row_load15_of_2_50_stops_the_stream_before_any_audio` |
| T-RP-83-033 | S83。`<proc-root>/4242/comm` を `cargo` と改行にする。`loadavg` は `0.50 0.50 0.50 1/300 1000` | 始める前の `cargo` | 0 | exit code 2。`stream.machine.refused` が 1 件、`reason` が `build`、値が `cargo` の 1。音声の byte は 0（specified、RP-83 の (2)。load が基準の内側でも build が在れば拒む） | 陰性 | `a_cargo_process_before_the_run_stops_the_stream` |
| T-RP-83-034 | 033 で `comm` を `rustc` と改行にする | `rustc` の側 | 0 | 033 と同じ。値が `rustc` の 1（specified、同上） | 陰性 | `a_rustc_process_before_the_run_stops_the_stream` |
| T-RP-83-035 | S83。`<proc-root>` に `4242/comm` = `cargo-nextest`、`4243/comm` = `mycargo`、`4244/comm` = `Cargo`、pid でない dir `sys/comm` = `cargo`（どれも改行つき）を置く | 数えない物（部分一致、大文字、pid でない dir） | `comm` が `cargo` か `rustc` に一致する pid の dir だけを数える | 始まる（031 と同じ 4 点）。`s83.machine.jsonl` の行 1 は `cargo` 0、`rustc` 0（specified、3.19 の「`/proc/<pid>/comm` がその名前に一致する process の数」。033 と 034 が数える側の陽性対照、この行が数えない側） | 境界、陽性 | `names_that_only_resemble_cargo_are_not_counted` |
| T-RP-83-036 | 033 に `4243/comm` = `cargo` を足す（`cargo` の process が 2 つ） | 数が 2 | 0 | exit code 2。`reason` が `build`、値が `cargo` の 2（derived、「process の数」を数える。1 つと 2 つを同じ値にしない） | 境界、陰性 | `two_cargo_processes_before_the_run_are_counted_as_two` |
| T-RP-83-037 | 存在しない旗 `--proc-rootx` を足して `stream` を起こす | exit code 2 が拒みでない場合（引数の誤りも 2） | 拒みの印は event | exit code は 2 だが `stream.machine.refused` は 0 件。偽の `/ingest` が受けた音声の byte は 0（derived、引数の誤りの exit code も 2 なので、拒みは exit code でなく event の 1 件で確かめる。030 から 034 の対照） | 陰性 | `a_usage_error_is_not_read_as_a_machine_refusal` |
| T-RP-83-038 | `hayamimi-eval stream --help` | 既定の `--proc-root` | 既定は `/proc` | 出力に `--proc-root` と `[default: /proc]` が在る（specified、RP-83 の検査の欄「`--proc-root` の既定は `/proc`」。旗の名前と既定の値を 1 行で固定する） | 陽性 | `proc_root_defaults_to_proc` |
| T-RP-83-039 | S83（wav は 12 秒）。始まった後、偽の `/ingest` が最初の音声を受けてから `<proc-root>/4242/comm` を `cargo` と改行にして足す | 送っている間に build が現れる | 次の行で止まる | exit code 3。足してから 8 秒以内に終わる（次の行は 5 秒ごと、残りは余裕 3 秒）。`stream.machine.aborted` が 1 件（level `warn`）で、`cargo` 1、`rustc` 0 の値を持ち、`stream.machine.refused` が 0 件。音声の byte は 040 の byte より小さい。`s83.machine.jsonl` の最後の行は `cargo` が 1 以上で、その file を RP-83 の判定に渡すと `Err` で `cargo` を名指す（specified、RP-83 の (3)。「次の行」は 5 秒ごとの行） | 陰性 | `a_cargo_process_during_the_run_aborts_the_stream_with_exit_code_3` |
| T-RP-83-040 | 039 と同じで、`cargo` の dir を足さない（12 秒の wav を最後まで送り、`--drain-s 11` で 11 秒待つ） | 039 の陽性対照。drain の間の行 | build が無い。drain の間も行が書かれる | exit code が 2 でも 3 でもない。`stream.machine.aborted` が 0 件。音声の byte は 039 より大きい。`s83.machine.jsonl` は 5 行以上（送り始めの前、送っている間の行が 2 行以上、drain の間の行、最後）で、`t_us` が増え、隣り合う行の差は 15 秒以下。**`t_us` が 12000000（音声の終わり）を超える行が 2 行以上**。その file を判定に渡すと `Ok`（specified、RP-83 の (3) の「drain の間を含む」と「記録」の 1。5 秒ごとの行が drain の間も書かれていることと、実際に書いた file が判定を通ることを固定する。drain の間の定期の行が止まる実装でも、最後の行は 1 行あるので、2 行以上を条件にして、最後の行のほかに drain の間の定期の行が 1 行以上あることを見る） | 陽性 | `a_run_beside_no_build_writes_rows_through_the_drain_and_a_file_that_passes` |
| T-RP-83-041 | 受入：023 の列、024 の列、009 の入力、016 の入力、020 の入力、042 の入力、045 の入力、046 の入力、056 の入力、057 の入力、063 の (a) の入力を順に判定に渡す | 全体 | 通る、落ちる、落ちる、落ちる、落ちる、落ちる、通る、落ちる、通る、落ちる、落ちる | 023 は `Ok`。024 は `Err` で `load15` を名指す。009 は `Err` で `cargo` を名指す。016 は `Err` で間の値 `20000000` を名指す。020 は `Err` で file が無いことを名指す。042 は `Err` で行が 2 行未満であることを名指す。045 は `Ok` で、出力に門の前の記録として外した 5 run の名前が在る。046 は `Err` で `rs1` を名指す。056 は `Ok`。057 と 063 の (a) は `Err` で `load5` と `3.10` を名指す（specified、RP-83 の (9)、(10) の陽性と陰性の case のうち判定の関数に渡す形が、受入の 1 本にも入る。RP-83 の陽性の case 1 つと、陰性の case のうち判定の関数に渡す形、(4) と (6)。`comm` が `cargo` の偽の process を `--proc-root` に置く陰性の case は、子の process を起こすので受入の 1 本に入れず、033 が持つ） | 陽性、陰性 | `rp83_a_run_under_a_busy_machine_or_beside_a_build_is_refused`（実装済み。main の `rp83.rs` に在る。この行に足した 056、057、063 の (a) の入力は未実装で、R8x-9 の席が `rp83.rs` に足す） |
| T-RP-83-042 | 001 の行 1 だけの file（1 行。`t_us` 0、全欄が基準の内側） | 行数の下限の下（029 の 2 行の 1 つ下） | 2 行以上 | `Err` で `rs1` と行が 2 行未満であることを名指す（specified、RP-83 の (4)「行が 2 行未満の file」。拒まれた run の file は 1 行なので、拒まれた run は判定を通らない。最初の行と最後の行が同じ行になる形を通す判定は、ここで落ちる。029 の 2 行が陽性対照） | 境界、陰性 | `a_machine_file_of_one_row_is_rejected` |
| T-RP-83-043 | S83。`loadavg` を `2.00 0.50 2.50 1/300 1000` にし、`<proc-root>/4242/comm` を `cargo` と改行にする（`load1`、`load15`、build の 3 つが同時に外れる） | `reason` の順の先頭（`load1`） | `load1`、`load15`、build の順 | exit code 2。`stream.machine.refused` が 1 件で、`reason` が `load1`。event は `load1` `2.00`、`load15` `2.50`、`cargo` 1、`rustc` 0 の 4 つの値を持つ。音声の byte は 0。`s83.machine.jsonl` は 1 行（specified、RP-83 の (5)「`reason` は `load1`、`load15`、`build` の順で外れた最初の 1 つで、event は 4 つの値を持つ」と (2)） | 境界、陰性 | `the_refusal_reason_is_load1_when_all_three_fail` |
| T-RP-83-044 | S83。`loadavg` を `0.50 0.50 2.50 1/300 1000` にし、`<proc-root>/4242/comm` を `cargo` と改行にする（`load15` と build が外れる） | `reason` の順の 2 番目（`load15`） | 同上 | 043 と同じで、`reason` が `load15`。event の値は `load1` `0.50`、`load15` `2.50`、`cargo` 1、`rustc` 0（specified、同上。033 は `load1` と `load15` が内側で build だけが外れる形なので、043、044、033 で `reason` の 3 つ全部が出る） | 境界、陰性 | `the_refusal_reason_is_load15_when_load15_and_build_fail` |
| T-RP-83-045 | 一時 dir を repo の root に見立て、`docs/results/rp32/` の `rs1.jsonl`、`rs2.jsonl`、`rs3.jsonl` と、`docs/results/e2e/e2e1/` の `e2e1.jsonl`、`docs/results/e2e/e2e2/` の `e2e2.jsonl` を、repo の実物から同じ相対 path で写す。5 つの Rust の run（`<run>.machine.jsonl` は無し）の列を判定に渡す | 門の前の記録の一覧に当たる run | 一覧に当たる run は外れる | `Ok`。出力に、5 つの run の名前と、門の前の記録として外したことが在る（specified、3.19 の「門の前の記録」と RP-83 の (6)。一覧は dir と run の名前と `<run>.jsonl` の sha256 で、pseudo：`sha256sum docs/results/rp32/rs1.jsonl docs/results/rp32/rs2.jsonl docs/results/rp32/rs3.jsonl docs/results/e2e/e2e1/e2e1.jsonl docs/results/e2e/e2e2/e2e2.jsonl` の出力が一覧の sha256。test は sha256 を手で書かず、実物の file を写して渡す） | 陽性 | `the_pre_gate_records_are_exempt_and_the_output_says_so` |
| T-RP-83-046 | 045 と同じだが、`rs1.jsonl` の写しの末尾に 1 byte（改行）を足す。5 つとも machine の file は無し | 名前が同じで sha256 が違う | 一覧に当たらない | `Err` で `rs1` と file が無いことを名指す。`rs2`、`rs3`、`e2e1`、`e2e2` は名指さない（specified、RP-83 の (6)「名前が同じで sha256 が違う run は外れない」。中身を差し替えた run を門の前の記録として通さない。045 が陽性対照） | 陰性 | `a_run_with_the_same_name_and_a_different_sha256_is_not_exempt` |
| T-RP-83-047 | 045 の `rs1.jsonl` の写しを 2 通り置く。(a) dir を `docs/results/rp32/` でなく `docs/results/other/` にし、名前は `rs1`。(b) dir は `docs/results/rp32/` で、名前を `rs4`（中身は `rs1.jsonl` と同じ bytes）にする | 一覧の（dir、名前、sha256）の dir と名前 | 3 つ組が全部一致 | どちらも `Err` で、その run の名前と file が無いことを名指す（derived、RP-83 の (6) の一覧は 3 つ組。sha256 だけで外す判定は、中身が同じ bytes の (a)(b) を通してしまう） | 境界、陰性 | `an_exemption_needs_the_dir_and_the_name_as_well_as_the_sha256` |
| T-RP-83-048 | 2 run の列：`rs1`（045 の写し。一覧に当たる。file 無し）と `rs4`（一覧に無い。file 無し） | 外れる run と外れない run の混在 | run ごと | `Err` で `rs4` と file が無いことを名指す。`rs1` は名指さない（derived、022 と同じ「外れた run だけを名指す」を、一覧に当たる run を含む列に掛ける） | 陰性 | `only_the_run_off_the_list_is_named_when_a_listed_run_is_present` |
| T-RP-83-049 | 001 で行 1 の `load15` を文字列 `"n/a"` にする | 読めない値（文字列だが数に読めない） | 2 桁の小数に読める | `Err` で `rs1` と `load15` を名指す（implicit、読めない値を 0 と読んで線の内側に通さない。T-RP-32-013 と同じ向き。028 は型が違う側、この行は文字列の中身が違う側） | 境界、陰性 | `an_unreadable_load_is_rejected_not_read_as_zero` |
| T-RP-83-051 | 045 の `rs1.jsonl` の写しに、`rs1.machine.jsonl` を付けた 2 入力。(a) T-RP-83-001 の形(内側)。(b) 001 で行 1 の `load15` を `"2.50"` にした形。`rs1` は一覧に当たる | 一覧に当たり、machine の file を持つ run | 免除は file が無いことだけに掛かる | (a) は `Ok`。(b) は `Err` で `rs1`、`load15`、`2.50` を名指す。免除の出力に `rs1` を載せない(specified、3.19 の「門の前の記録」と H127 の 7。file が在る run は一覧に当たっても他の run と同じに判定される。一覧に当たる run を file の有無によらず外す build は、(b) を通して落ちる) | 境界、陰性 | `a_listed_run_with_a_machine_file_is_judged_like_any_other` |
| T-RP-83-052 | 045 の `rs1.jsonl` の写しだけの 1 run の列(Rust、file 無し、一覧に当たる) | 一覧に当たり、file を持たない run | 免除は file が無いことに掛かる | `Ok`。出力に `rs1` を門の前の記録として外したことが在る(specified、同上。051 の file が在る側と対) | 陽性 | `a_listed_run_without_a_machine_file_is_exempt` |
| T-RP-83-050 | 001 で行 1 の `load1` を JSON の `null` にする | `null`（欄は在るが値が無い） | 3 つの load は JSON の文字列 | `Err` で `rs1` と `load1` を名指す（implicit、`null` を 0 と読んで線の内側に通さない。欄の欠け（027）とは別の形） | 境界、陰性 | `a_null_load_is_rejected_not_read_as_zero` |
| T-RP-83-053 | S83。`<proc-root>/pressure/io` を `some avg10=0.00 avg60=0.00 avg300=0.00 total=12345` と `full avg10=0.00 avg60=0.00 avg300=0.00 total=678` の 2 行(`/proc/pressure/io` の書式)にし、`loadavg` は内側の値で `stream` を起こす | pressure が読める kernel | 全行に 2 欄が入る | exit code 0。`s83.machine.jsonl` の全行が `io_some_total` 12345 と `io_full_total` 678 を持つ(JSON の整数、単位はマイクロ秒)。`stream.machine.pressure_unavailable` は 0 件(specified、RP-83 の (8)「記録だけの項」。線は置かない。054 の対) | 陽性 | `rows_carry_the_io_pressure_totals_when_the_kernel_shows_them` |
| T-RP-83-054 | S83。`<proc-root>` に `pressure/io` を置かず(読めない kernel)、`loadavg` は内側の値で `stream` を起こす。同じ `s83.machine.jsonl` を T-RP-83-001 の形の run として判定に渡す | pressure が読めない kernel | 全行が `null`、event が 1 件 | exit code 0。`s83.machine.jsonl` は 2 行以上で、全行の `io_some_total` と `io_full_total` が JSON の `null`。`stream.machine.pressure_unavailable` が 1 件(level `warn`)。判定は `Ok`(specified、同上。判定は 2 欄を読まないので、`null` の行は通る。読めない kernel を拒みの理由にしない) | 境界、陽性 | `rows_carry_null_and_one_event_says_so_when_the_kernel_hides_the_pressure` |
| T-RP-83-055 | 2 入力。(a) 001 で行 2 の `io_some_total` を文字列 `"x"`、行 3 の `io_full_total` を負の数 `-1`、行 4 の `io_some_total` を小数 `1.5` にする(型が違う値)。(b) 001 の全行に `io_some_total` 12345 と `io_full_total` 678(JSON の整数)を入れる(053 の形の実物の file) | 判定が 2 欄の値を読まない | 2 欄は判定に入らない | どちらも `Ok`(specified、RP-83 の (8)「記録だけの項。線は置かない」。2 欄が欠けた行と `null` の行は 001 と 054 が持つので、この行は型が違う値と実物の整数だけを持つ。2 欄を `u64` として型つきで読む build は (a) で落ちる。(b) が通ることは、2 欄を読まない判定が整数の行を拒まない側の陽性対照) | 境界、陽性 | `the_judgement_ignores_the_values_of_the_pressure_columns`（未実装、持ち主 R8x-1） |
| T-RP-83-056 | O83、c = 370 | `own5` 0.611407、切り捨て 61。引いた後 249 | 引いた後 < 250 | `Ok`（specified、RP-83 の (9)「最後の行の `load5` の 100 倍から `own5` の 100 倍の切り捨てを引いた整数が 250 未満」。310 − 61 = 249。pseudo は O83 の式） | 境界、陽性 | `a_load5_of_3_10_with_an_own_share_of_0_61_passes` |
| T-RP-83-057 | O83、c = 364 | `own5` 0.601493、切り捨て 60。引いた後 250 | 同上 | `Err` で `rs1`、`load5`、`3.10` を名指す（specified、同上。250 は 250 未満でない。056 の 1 つ下の陰性） | 境界、陰性 | `a_load5_of_3_10_with_an_own_share_of_0_60_is_rejected` |
| T-RP-83-058 | O83、c = 369 | `own5` 0.609755（0.609 台）。四捨五入すれば 61 | 同上 | `Err` で `rs1`、`load5`、`3.10` を名指す（specified、同上「切り捨てる」。100 倍の切り捨ては 60 で、310 − 60 = 250。四捨五入か切り上げで 61 にする build は 249 で通してしまい、ここで落ちる。056 が 0.61 の側の陽性対照） | 境界、陰性 | `an_own_share_of_0_609_is_cut_to_60_not_rounded_to_61` |
| T-RP-83-059 | 2 行版の行 2 の `load5` を `"2.97"` にする | 引いた後 249。行の間 15 秒 | 同上 | `Ok`（derived、`own5` 0.487706、切り捨て 48、297 − 48 = 249。`e^(−Δt/300)` と率の Δt に、行の間の実際の秒（15）を使う判定だけが 48 になる。5 秒と決め打つ build は `own5` が 0.495856 で 49 になる） | 境界、陽性 | `a_two_row_file_with_an_own_share_of_0_48_and_a_load5_of_2_97_passes` |
| T-RP-83-060 | 2 行版の行 2 の `load5` を `"2.98"` にする | 引いた後 250 | 同上 | `Err` で `rs1`、`load5`、`2.98` を名指す（derived、298 − 48 = 250。Δt を 5 秒と決め打つ build は 298 − 49 = 249 で通し、四捨五入の build も 49 で通すので、どちらもここで落ちる。059 が 1 つ下の陽性対照） | 境界、陰性 | `a_two_row_file_with_an_own_share_of_0_48_and_a_load5_of_2_98_is_rejected` |
| T-RP-83-061 | O83 で `own_cpu_ticks` を全行 0 にする（c = 0。最後の行の `load5` は `"3.10"`） | run 自身の分が 0 | 同上 | `Err` で `rs1`、`load5`、`3.10` を名指す（specified、RP-83 の陰性の case「最後の行の `load5` が `"3.10"` で `own_cpu_ticks` が全行で同じ（run 自身の分 0）fixture が (9) で落ちる」。310 − 0 = 310） | 陰性 | `a_load5_of_3_10_with_no_own_work_is_rejected` |
| T-RP-83-062 | 2 入力。(a) O83 で c = 0 にし、行 106 の `load5` を `"2.11"` にする。(b) 窓 E3' の `e3p1.e2e1.machine.jsonl` の実物の写し（198 行。`own` を持たない。最後の行の `load5` は `"2.11"`） | ja の `load5` 2.11 は、run 自身の分が 0 でも通る（対照） | 同上 | どちらも `Ok`（specified、3.21 の 3「ja の 2 本は同じ門の下で 2.11 で通り」と RP-83 の (9)、(10)。(a) は 211 − 0 = 211 < 250。(b) は古い形の file が今のまま通る。`own_cpu_ticks` が全行同じは「前の行より小さい」に当たらない（064 の等号側）。pseudo：`jq -s -c '{rows:length,last:.[-1].load5}' docs/results/r8x/e3p1.e2e1.machine.jsonl` が `{"rows":198,"last":"2.11"}`。2026-10-21 に打った） | 境界、陽性 | `a_load5_of_2_11_passes_with_an_own_share_of_zero` |
| T-RP-83-063 | 2 入力。(a) 窓 E3' の `e3p1.e2e2.machine.jsonl` の実物の写し（106 行。`own` を持たない。最後の行の `load5` は `"3.10"`）。(b) O83、c = 370 から、`own`、`clk_tck`、`own_cpu_ticks`、`cpu_some_total` の欄を全行から除いた物 | `own` を持たない古い形 | 引かずに今の形で判定する | どちらも `Err` で、その run の名前、`load5`、`3.10` を名指す（specified、RP-83 の (10) と 3.21 の 3 の 5「`e3p1.e2e2` の判定は変わらない（落ちたまま）」。(b) は 056 と同じ中身から欄を除いた形で、056 が通ることとの対が、欄の有無で判定が変わることを示す。pseudo：`jq -s -c '{rows:length,own:(map(select(has("own")))\|length),last:.[-1].load5}' docs/results/r8x/e3p1.e2e2.machine.jsonl` が `{"rows":106,"own":0,"last":"3.10"}`。2026-10-21 に打った） | 陰性 | `a_file_without_own_is_judged_without_subtracting` |
| T-RP-83-064 | O83、c = 370 で、行 50 の `own_cpu_ticks` を行 49 の値より 1 小さくする（行 51 以降は元の値） | 前の行より小さい行（境界は −1） | 前の行以上 | `Err` で `rs1`、行 50、`own_cpu_ticks` を名指す。行 51 は名指さない（specified、RP-83 の (9)「`own_cpu_ticks` が前の行より小さい行は、その行を名指して落ちる」。等号側は 061 と 062 の (a) が通る） | 境界、陰性 | `an_own_cpu_ticks_below_the_previous_row_is_named_by_its_row` |
| T-RP-83-065 | 3 入力。O83、c = 370 で、(a) 行 60 から `own_cpu_ticks` の欄を除く。(b) 行 60 の `own_cpu_ticks` を JSON の文字列 `"22200"` にする。(c) 行 60 の `own_cpu_ticks` を JSON の `null` にする | 欄の欠け、型の違い、`null` | JSON の整数 | どれも `Err` で `rs1`、行 60、`own_cpu_ticks` を名指す（implicit、読めない値を 0 や前の行の値と読んで通さない。T-RP-83-027、028、050 と同じ向き。056 が同じ中身の陽性対照） | 境界、陰性 | `an_own_cpu_ticks_that_is_missing_or_not_an_integer_is_rejected` |
| T-RP-83-066 | O83、c = 370 で、行 1 から `clk_tck` の欄を除く | 行 1 の欄の欠け | `own` を持つ file は `clk_tck` も持つ | `Err` で `rs1` と `clk_tck` を名指す（implicit、設計は行 1 が `own` と `clk_tck` を持つと書く。`clk_tck` を 100 と決め打って通さない。056 が陽性対照） | 境界、陰性 | `a_first_row_with_own_but_without_clk_tck_is_rejected` |
| T-RP-83-067 | `/proc/<pid>/stat` の text 1 本から値を読む関数に、3 入力を渡す。(a) `4242 (hayamimi) S 1 4242 4242 0 -1 4194560 100 0 0 0 1000 250 0 0 20 0 8 0 12345 1000000 100 18446744073709551615`。(b) comm に空白と `)` を含む `4242 (a b) c) S 1 4242 4242 0 -1 4194560 100 0 0 0 1000 250 0 0 20 0 8 0 12345 1000000 100 18446744073709551615`。(c) 13 番目の欄で終わる `4242 (hayamimi) S 1 4242 4242 0 -1 4194560 100 0 0 0` | 欄の位置（14 番目 utime、15 番目 stime）。comm の中の空白と括弧。短すぎる text | utime + stime | (a) は 1250、(b) は 1250、(c) は `Err`（derived、proc_pid_stat(5)：(2) comm は括弧で囲まれ、(14) utime と (15) stime はクロックティック。最後の `)` の後ろから数える。comm を空白で割って数える build は (b) でずれる。(c) は欠けを 0 と読まない。2026-10-21 に man page を読んだ。判定の関数の名前と引数の形は実装の席が決める） | 境界、陽性、陰性 | `own_cpu_ticks_of_one_stat_text_is_utime_plus_stime` |
| T-RP-83-068 | S83o（wav は 1 秒）で起こす。`cmdline` を持つのは 4244 だけ | 3 つの pid の組。comm が `hayamimi`、`hayamimi-relay` の pid は `cmdline` が無くても受けられる | 全部の pid が受けられ、3 つの comm がちょうど 1 つずつ | 始まる。exit code が 2 でも 3 でもない、`stream.machine.refused` と `stream.machine.aborted` が 0 件、偽の `/ingest` が受けた音声の byte が 1 以上。`s83.machine.jsonl` の行 1 の `own` は `[{"pid":4242,"comm":"hayamimi"},{"pid":4243,"comm":"hayamimi-relay"},{"pid":4244,"comm":"llama-server"}]` に等しい（渡された順。`stream` 自身の pid は足されない）。`clk_tck` が 100。全行の `own_cpu_ticks` が JSON の整数 3751（1250 + 700 + 1801。S83o の stat の utime と stime の和の合計）。その file を判定に渡すと `Ok`（specified、RP-83 の (9) と 3.21 の 3 の 1「他の pid は comm だけで受ける」「`stream` 自身の pid は足さない」。pid ごとに utime と stime の割合を変えてあるので、1 つの pid だけ、utime だけ、同じ pid を 3 回読む build は 3751 にならない） | 陽性 | `an_own_pid_set_of_the_three_processes_starts_the_stream_and_is_recorded` |
| T-RP-83-069 | 068 に `--own-pid 4246` と `4246/comm` = `sudo`（`stat` も置く）を足す | comm が `sudo`（`sudo -n nice -n -5` で起こした process の `$!` は `sudo` の pid） | 拒む | exit code 2。`stream.machine.refused` が 1 件（level `warn`）で `reason` が `own_pid`、`own_fault` が `comm`、`own_pid` が 4246（JSON の整数）。event は `load1`、`load15`、`cargo`、`rustc` の 4 つの値も持つ。偽の `/ingest` が受けた音声の byte は 0（specified、RP-83 の (5)、(9)「comm が `sudo` の pid を `--own-pid` に渡すと `stream` が exit code 2（`reason` `own_pid`）を返し、送った byte が 0」と 3.21 の 3 の 1 の event。068 が同じ組から 4246 を除いた陽性対照） | 陰性 | `a_sudo_pid_stops_the_stream_before_any_audio` |
| T-RP-83-070 | 068 に `--own-pid 4246` と `4246/comm` = `bash`（`stat` も置く）を足す | comm が受ける 3 つ（`hayamimi`、`hayamimi-relay`、`llama-server`）のどれでもない | 拒む | 069 と同じ。`own_fault` は `comm`、`own_pid` は 4246（specified、RP-83 の (9)「comm が `hayamimi`、`hayamimi-relay`、`llama-server` のどれでもない pid」。`sudo` 以外の名前も拒む） | 陰性 | `a_pid_with_another_comm_stops_the_stream_before_any_audio` |
| T-RP-83-071 | 068 の `4244/cmdline` の `18084` を `8080` にする | comm が `llama-server` で、別の port | 拒む | 069 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified、RP-83 の (9) と 3.21 の 3 の 1「別の app の llama-server（8080 など）の pid を渡すと、run の外の負荷を run 自身の分として引き、門を通しすぎる」。068 が `18084` の陽性対照） | 陰性 | `a_llama_server_on_port_8080_stops_the_stream_before_any_audio` |
| T-RP-83-072 | 2 入力。068 の `4244/cmdline` の `18084` を、(a) `180840` にする。(b) `118084` にする | `18084` を前や後ろに含む別の語（完全一致でない） | `--port` の次の語が `18084` に等しい | どちらも 069 と同じ。`own_fault` は `port`、`own_pid` は 4244（derived、`--port` の次の語を完全一致で比べる。語は NUL 区切り（proc_pid_cmdline(5)）。部分一致か前方一致で受ける build は、ここで受けて落ちる） | 境界、陰性 | `a_llama_server_on_a_port_that_only_contains_18084_stops_the_stream` |
| T-RP-83-073 | 068 の `4244/cmdline` を `llama-server`、`-m`、`model.gguf`、`--port` の 4 語にする（最後の語が `--port` で、その次の語が無い） | `--port` が最後の語 | 次の語が要る | 069 と同じ。`own_fault` は `port`、`own_pid` は 4244（derived、(9) の「`--port` の次の語が `18084`」。次の語が無い `--port` を、`18084` を持つとは読まない） | 境界、陰性 | `a_port_flag_with_no_value_stops_the_stream` |
| T-RP-83-074 | 2 入力。068 の `4244/cmdline` を、(a) `llama-server`、`--port`、`8080`、`--parallel`、`18084` にする。(b) `llama-server`、`18084`、`--port`、`8080` にする | `18084` が語として在るが、`--port` の次の語でない | `--port` の次の語 | どちらも 069 と同じ。`own_fault` は `port`、`own_pid` は 4244（derived、同上。cmdline の中に `18084` の字が在れば受ける build は、ここで受けて落ちる） | 境界、陰性 | `a_18084_that_is_not_the_value_of_port_stops_the_stream` |
| T-RP-83-075 | 2 入力。068 の `4244/cmdline` を、(a) 置かない。(b) 0 byte の file にする（zombie の cmdline は 0 文字。proc_pid_cmdline(5)） | cmdline が読めない。空 | 拒む | どちらも 069 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified、RP-83 の (9)「cmdline が読めない、空」。読めない物と空は「無い」。comm だけで受ける build は、ここで受けて落ちる） | 境界、陰性 | `an_unreadable_or_empty_cmdline_stops_the_stream` |
| T-RP-83-076 | 3 入力。068 に `--own-pid 4250` を足し、(a) `4250` の dir を置かない。(b) `4250/comm` だけを置き、`stat` を置かない。(c) `4250/stat` だけを置き、`comm` を置かない | 読めない pid。comm と stat のどちらが欠けても | 2 つとも読める | 3 入力とも 069 と同じ。`own_fault` は `unreadable`、`own_pid` は 4250（specified、RP-83 の (9)「comm か stat が読めない pid」。(b) は `stat` を置かない pid を受ける build を落とす。068 が陽性対照） | 陰性 | `a_pid_that_cannot_be_read_stops_the_stream` |
| T-RP-83-077 | 3 入力。068 から、(a) `--own-pid 4242`。(b) `--own-pid 4243`。(c) `--own-pid 4244` を除く（残る pid の dir は在る） | 3 つのうち 1 つが欠けた組 | 3 つが揃う | 3 入力とも 069 と同じ。`own_fault` は `set`、`own_pid` は JSON の `null`（specified、RP-83 の (9)「3 つの comm がちょうど 1 つずつでない組」。残りの pid は読めて受けられる形なので、拒む理由は組の欠けだけ） | 境界、陰性 | `a_set_missing_one_of_the_three_processes_stops_the_stream` |
| T-RP-83-078 | 068 に `--own-pid 4247` を足し、`4247/comm` = `llama-server`、`4247/stat`（置く）、`4247/cmdline` を `--port 8080` を持つ形にする（`4244` は `18084` のまま。3 つの組は揃っている） | 揃った組に、受けられない pid が 1 つ | 全部の pid が受けられる | 069 と同じ。`own_fault` は `port`、`own_pid` は 4247（derived、RP-83 の (9) の拒みを pid ごとに読み、組の確かめ（`set`）は全部の pid が通った後に打つ。組が揃っていれば他の pid の不適格を見逃す build は、ここで受けて落ちる） | 境界、陰性 | `one_bad_pid_beside_a_complete_set_stops_the_stream` |
| T-RP-83-079 | 068 の起動の引数に `--own-pid abc` を足す | 数でない pid（引数の誤り） | 拒みの印は event | exit code は 2 だが `stream.machine.refused` は 0 件。偽の `/ingest` が受けた音声の byte は 0（derived、T-RP-83-037 と同じ。引数の誤りの exit code も 2 なので、拒みは event の 1 件で確かめる） | 陰性 | `a_non_numeric_own_pid_is_a_usage_error_not_a_refusal` |
| T-RP-83-080 | 3 入力。(a) 068 の `<proc-root>/pressure/cpu` を `some avg10=0.00 avg60=0.00 avg300=0.00 total=777` の 1 行にして起こす。(b) `pressure/cpu` を置かずに起こす。(c) 056 の file の全行の `cpu_some_total` を JSON の文字列 `"x"` にして判定に渡す | 記録だけの項。読める kernel、読めない kernel、型が違う値 | 判定に入らない | (a) は `s83.machine.jsonl` の全行が `cpu_some_total` 777（JSON の整数）。(b) は全行が JSON の `null`。(c) は `Ok`（specified、RP-83 の (9)「記録だけの `cpu_some_total`（読めなければ null）」。判定は読まない。T-RP-83-053 から 055 と同じ向き） | 境界、陽性 | `rows_carry_the_cpu_pressure_total_and_the_judgement_ignores_it` |
| T-RP-83-081 | 2 入力。068 に `--own-pid 4246` を足し、`4246/comm` を (a) `hayamimi-foo`、(b) `hayamimi-relay2` にする（どちらも `stat` を置く。3 つの組は揃っている） | comm が受ける名前で始まるが、完全一致でない | comm は 3 つの名前に完全一致 | どちらも 069 と同じ。`own_fault` は `comm`、`own_pid` は 4246（derived、RP-83 の (9)「comm が `hayamimi`、`hayamimi-relay`、`llama-server` のどれでもない pid」で、受ける名前は 3 つの完全一致。前方一致で受ける build は、ここで受けて落ちる。068 が完全一致の陽性対照） | 境界、陰性 | `a_comm_that_only_starts_like_ours_stops_the_stream` |
| T-RP-83-082 | 068 に `--own-pid 4245` と `4245/comm` = `hayamimi-eval`（`stat` も置く）を足す | comm が `hayamimi-eval`（`stream` 自身の名前。渡された pid としては受けない） | comm は 3 つの名前のどれか | 069 と同じ。`own_fault` は `comm`、`own_pid` は 4245（specified、3.21 の 3 の 1 の表の 2 と RP-83 の陰性欄「comm が `hayamimi-eval` の pid」。`stream` 自身の pid は足されないので、`hayamimi-eval` を受ける理由が無い。受ける build は、ここで受けて落ちる） | 境界、陰性 | `a_hayamimi_eval_pid_stops_the_stream_with_the_comm_fault` |
| T-RP-83-083 | 068 の `4244/cmdline` を `llama-server`、`--port=18084` の 2 語にする | `=` でつなぐ 1 語の形 | `--port` の次の語が `18084` | 069 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified、3.21 の 3 の 1「`--port=18084` の 1 語の形は外れ」。窓の訳の server の unit は `=` の形を生まないので、受けない側に倒す） | 境界、陰性 | `a_port_flag_joined_with_equals_stops_the_stream` |
| T-RP-83-084 | 2 入力。068 の `4244/cmdline` を、(a) `llama-server`、`--port`、`18084`、`--port`、`18084` にする。(b) `llama-server`、`--port`、`8080`、`--port`、`18084` にする | `--port` が 2 つ。2 つとも `18084` の形と、後ろだけが `18084` の形 | `--port` がちょうど 1 つ | どちらも 069 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified、3.21 の 3 の 1「語 `--port` がちょうど 1 つ」。(b) は最後の `--port` だけを見る build を落とし、(a) は `18084` に等しければ数を見ない build を落とす） | 境界、陰性 | `two_port_flags_stop_the_stream` |
| T-RP-83-085 | 4 入力。(a) `loadavg` を `2.00 0.50 0.50 1/300 1000` にして、068 に `--own-pid 4246`（`sudo`）を足す。(b) `loadavg` を `0.50 0.50 2.50 1/300 1000` にして、同じ pid を足す。(c) `loadavg` は内側の値で、`<proc-root>/4260/comm` を `cargo` にして、同じ pid を足す。(d) `loadavg` を `2.00 0.50 0.50 1/300 1000` にして、068 の 3 つの pid だけを渡す | `own_pid` と、`load1`、`load15`、build の同時の外れ。(d) は `own_pid` が外れない対照 | `reason` は `own_pid`、`load1`、`load15`、`build` の順で最初の 1 つ | (a)(b)(c) は exit code 2、`reason` が `own_pid`、`own_fault` が `comm`、`own_pid` が 4246。event は `load1`、`load15`、`cargo`、`rustc` の 4 つの値も持つ（(a) は `load1` が `2.00`、(b) は `load15` が `2.50`、(c) は `cargo` が 1）。音声の byte は 0。`s83.machine.jsonl` は 1 行。(d) は `reason` が `load1` で、event は `own_fault` と `own_pid` を持たない（specified、3.21 の 3 の 1「拒みの理由の順」と「event」。引数の誤りは待っても直らないので先に拒む。043 と 044 の型の入力に `own_pid` を足した形） | 境界、陰性 | `the_refusal_reason_is_own_pid_when_load_fails_at_the_same_time` |
| T-RP-83-086 | 2 入力。起こす引数の `--own-pid` の並びを、(a) 4246（comm が `sudo`）、4250（dir が無い）、4242、4243、4244 の順にする。(b) 4250、4246、4242、4243、4244 の順にする | 外れた pid が 2 つ。並びの順 | 渡された順に確かめ、最初に外れた pid で拒む | (a) は `own_fault` が `comm`、`own_pid` が 4246。(b) は `own_fault` が `unreadable`、`own_pid` が 4250。どちらも exit code 2、`reason` が `own_pid`、音声の byte は 0（specified、3.21 の 3 の 1「渡された pid を渡された順に 1 つずつ確かめ、最初に外れた pid で拒む」。外れの種類で先に拒む build は、(a)(b) の片方で落ちる） | 境界、陰性 | `the_first_failing_pid_in_the_given_order_names_the_fault` |
| T-RP-83-087 | S83（`--own-pid` も `--own-llama-port` も渡さない。`<proc-root>/pressure/cpu` を `some avg10=0.00 avg60=0.00 avg300=0.00 total=777` の 1 行にする）を、wav 1 秒で起こす | `--own-pid` が 0 個 | 今のまま動く | 始まる。exit code が 2 でも 3 でもない、`stream.machine.refused` が 0 件、音声の byte が 1 以上。`s83.machine.jsonl` の全行が `own`、`clk_tck`、`own_cpu_ticks`、`cpu_some_total` の欄を持たない（欄が無い。`null` ではない）。その file を判定に渡すと `Ok`（specified、3.21 の 3 の 1「0 個」と RP-83 の (9)、(10)。030 から 040、043、044、053、054 が `--own-pid` を渡さないまま今の期待値で通ることの根拠。068 が同じ中身に 3 つの pid を足した形で、欄が現れる対照） | 境界、陽性 | `a_stream_without_own_pid_starts_and_writes_rows_without_own_fields` |
| T-RP-83-088 | 2 入力。S83o（wav は 12 秒、`--drain-s 1`）で、偽の `/ingest` が最初の音声を受けてから、(a) `<proc-root>/4242/stat`（先頭の pid）を除く。(b) `<proc-root>/4244/stat`（先頭でない pid）を除く | 送っている間に、run の process の stat が読めなくなる | 止めない。その行が null | exit code が 2 でも 3 でもない。`stream.machine.refused` と `stream.machine.aborted` が 0 件。`s83.machine.jsonl` は、行 1 の `own_cpu_ticks` が 3751 で、除いた後の最初の行から最後の行まで `own_cpu_ticks` が JSON の `null`。その file を判定に渡すと `Err` で、最初の `null` の行（行番号）と `own_cpu_ticks` を名指す（specified、3.21 の 3 の 1「2 行目より後で、どれか 1 つの pid の stat が読めない行は `own_cpu_ticks` を null にし、`stream` は止めない」と 3「null の行は、その行を名指して落ちる」。どれか 1 つの pid でよいので、先頭の pid だけを見て null にする build は (b) で落ちる。行 1 の読めない pid は `unreadable` で拒む（076）ので、null は 2 行目より後だけ） | 陰性 | `a_pid_whose_stat_vanishes_mid_run_writes_a_null_row_and_does_not_stop_the_stream` |
| T-RP-83-089 | 4 入力を判定に渡し、出力の行を読む。(a) 056 の run `rs1`。(b) 057 の run `rs1`。(c) 063 の (a) の run `e3p1.e2e2`。(d) 2 run の列：(a) の `rs1` と (b) の `rs2` | 判定の出力の 1 行。`own` を持つ file、持たない file、`Ok` と `Err` | 1 run に 1 行 | (a) は `Ok` で、出力に `rs1 load5 3.10 own5 0.611407 (61) judged 249 limit 250` の行が 1 本。(b) は `Err` で、`rs1 load5 3.10 own5 0.601493 (60) judged 250 limit 250` の行が 1 本。(c) は `Err` で、`e3p1.e2e2 load5 3.10 own absent judged 310 limit 250` の行が 1 本。(d) は `rs1` の行と `rs2` の行が 1 本ずつで、`rs2` の行は (b) の値（specified、3.21 の 3 の 7 と RP-83 の (9)。値は O83 の式（pseudo）と 063 の jq の出力から打った。`Ok` でも `Err` でも出す。判定の関数が行をどう返すかは実装の席が決め、test はその行の字を読む） | 境界、陽性、陰性 | `the_judgement_prints_one_line_per_run_with_own5_and_the_judged_value` |
| T-RP-83-090 | 3 入力。068 に、(a) `--own-pid 4248` と `4248/comm` = `hayamimi`（`stat` も置く）を足す。(b) `--own-pid 4249` と `4249/comm` = `llama-server`、`4249/cmdline` を `llama-server`、`--port`、`18084` にしたもの（`stat` も置く）を足す。(c) 068 から `--own-pid 4243` を除き、`--own-pid 4248` と `4248/comm` = `hayamimi`（`stat` も置く）を足す（pid は 3 つで、comm は `hayamimi` が 2 つと `llama-server`） | 全部の pid が通り、同じ comm が 2 つ | 3 つの comm がちょうど 1 つずつ | 3 入力とも 069 と同じ。`own_fault` は `set`、`own_pid` は JSON の `null`（specified、3.21 の 3 の 1 の表の 4 と RP-83 の陰性欄「`hayamimi` の pid を 2 つ渡した組」。077 は欠けた側、この行は重なった側。pid の数が 3 つかだけを見る build は、(c) で通して落ちる） | 境界、陰性 | `two_pids_of_the_same_process_are_a_set_fault` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 一覧の dir を判定がどの基準で引くかと、それを渡す引数の形は H128(R8x-1 の席が決める)。045 から 047 と 051、052 は、repo の root に見立てた一時 dir を判定へ渡す形で書いた。
- 途中の打ち切りで `rustc` の process が現れる形は走らせない（13 秒かかる行を増やさない）。`cargo` と `rustc` は同じ数え方（034 と 010、012）を通る。
- 056 から 090 は、3.21 の 3 の 1、3、7 で決まった形で書いた（`--own-pid` の 0 個は今のまま動き、行は `own` 等を持たない：087。`stream` 自身の pid は足さない：068。`own` の要素は `{"pid","comm"}`：O83。`own_fault` の 4 語：069 から 078、081 から 084、090。`reason` の順 `own_pid`、`load1`、`load15`、`build`：085。`--port=18084` は拒む：083。`--port` が 2 つは拒む：084。途中の行の stat の null：088 と 065。判定の出力の 1 行：089）。
- `stream` を `--own-pid` つきで起こす行は 068 から 086、088、090 で、S83o の引数の `--own-llama-port 18084` を持つ（窓の形。設計 10.7.1）。056 から 067 は判定や stat の読みの行、089 は判定の出力の行で、`stream` を起こさない。087 は `--own-pid` も `--own-llama-port` も渡さない。「`--own-llama-port` の欠けで exit 2」と「`--own-pid` 無しの `--own-llama-port` で exit 2」は RP-83 に足さず、4.86 の T-IP-08-004 と 005 が持つ。
- 設計の文のうち、case 行にしていない物：
  1. H138 の unit test（自分の process の中で CPU を使う thread を 1 本走らせる。その thread は自分の `/proc/thread-self/stat` の utime と stime の和が 1 tick 以上になるまで回し、終わる直前にその和を読む。join の後、`/proc/self/stat` の和が `/proc/self/task/*/stat` の和と終わった thread の値の和以上であることを見る。実の `/proc` を読むだけで、file を書かず、子 process も起こさない）は、受入 ID の行ではなく R8x-9 の席の test である。
  2. H142（llama-server が argv を書き換えないこと）と H143（`stream` 自身の CPU を引かない量）は、窓の席が報告に書く事で、test の行にならない。
  3. 拒んだ時の最初の行が `own` を持つか。設計は「最初の行は今と同じく 1 行書く」とだけ書くので、085 は行数だけを見る。

### 4.76 RP-84（認識器の初回の復号を起動の時に済ませる）

**001 から 025 は `native/hayamimi-eval/tests/rp84.rs` の 25 本と 1 対 1 で実装済み**（`feat/r8x-3`。24 本が通り、023 は `#[ignore]`）。**E3'' の名前（`e3q<N>`）と 3.21 の数え方に直した行は、実装が追いついていない。** 001 から 025 の run の名前を `e3q` にした直しと、足した 026 から 037（`punct_ms`、`e3p` の記録を読まない対照、名前の境界）は、R8x-7 の席が `rp84.rs` を直すまで通らない。関数の末尾は 001 から 025 のまま変えない（実装との 1 対 1 を、R8x-7 の席が直すまで保つため。021 の末尾 `runs_not_named_e3p_are_not_read_for_the_first_utterance` は旧名の字を含むが、関数の名前であって、読む run の名前ではない）。fixture は `tests/fixtures/rp84_recorded/`（`rs1`〜`rs3` の 5 欄の写しと、`rs1` の stdout の写し）に在る。起動の run が `asr.warmup.decoded` を 1 件、`asr.model.loaded` の後かつ最初の `asr.endpoint.decided` の前に出し、`audio_s` が 10、`decode_ms` が 0 を超え、`asr.warmup.failed` が 0 件であることは、`native/hayamimi/tests/rp84_start.rs`（実走の 1 本が `#[ignore]`）が数える。
test：`native/hayamimi-eval/tests/rp84.rs`。受入 test の名前は `rp84_the_first_utterance_decodes_as_fast_as_the_rest`。実走の行（T-RP-84-023）だけが `#[ignore]` を持ち、`docs/results/r8x/` の E3'' の run(`e3q<N>.e2e1`)を読む。記録が置かれるまでは走らせない。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp84`。実走の行はこれに `-- --ignored` を足す。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない（023 だけは受入 test の名前の全部）。判定の関数の名前と引数の形は実装の席が決める。行は「E3'' の run の列（名前、標本の列、`hayamimi` の stdout の text）を渡す」形で書く。run の名前が `e3q` と数字だけの物を E3'' の run と読む(`is_x_run`。feat/r8x-3 の実装は `e3p` と数字だけを読んでいたので、R8x-7 の席が `e3q` の 1 つの定数に替える)。`e3p` と数字だけの名前は E3'' の run ではなく、読まない（026 から 037 の対照）。窓 E3' の `e3p` の記録は `docs/results/r8x/` に残り、どの test も読まない。
設計は D 3.19 の「認識器の初回の復号」と、D 6 と 7 の RP-84、D 10 の H117 のそばの ASSUMPTION（作業域）である。

**数え方**：

- `d = (t_final_us − t_end_us) / 1000 − final_latency_ms`。有効は `t_final_us` が在り `d ≥ 120 ms`（3.19 の「数えた結果」の有効。RP-66 の線と同じ。RP-84 の「他の有効な標本」もこの線で数える）。発話 1 の 1 回目（A）は、`final` が在れば `d` によらず比べる（`d` が 120 ms 未満でも比べる）。`final` が無ければ落ちる。
- 比べる集合は、同じ run の有効な標本から「発話 1 の 1 回目」の 1 標本を除いた物。p95 と p50 は nearest-rank で、順位は (p × n + 99) / 100 の整数除算。
- `final_latency_ms` は小数 6 桁まで持つので、10 の -6 乗の整数に読んで比べる。
- stdout の行は 1 行 1 つの JSON で、`event` の値が `asr.warmup.decoded`、`asr.warmup.failed`、`asr.endpoint.decided` の行を数える。「前」は stdout の行の順である。

**F84**（式で作る。実測のふりをしない）：E3'' の run 1 本。枠 115 = 23 発話 × 5 回で、全標本が有効（`d` は 500 ms）。発話 1 の 1 回目を A とする。A のほかの 114 標本の `final_latency_ms` は、101、102、…、214 ms（j 番目が 100 + j ms）。したがって比べる集合は n = 114、p95 の順位は (95 × 114 + 99) / 100 = 109 で 209 ms、p50 の順位は (50 × 114 + 99) / 100 = 57 で 157 ms。
**F84b**：F84 の 114 標本のうち 6 標本を、`d` 100 ms（終端不正）で `final_latency_ms` 5000 ms の標本にし、残りの 108 標本を 101〜208 ms にした物。有効な比べる集合は n = 108、p95 の順位は (95 × 108 + 99) / 100 = 103 で 203 ms。6 標本を集合に入れると n = 114 の 109 番目が 5000 ms になる。
**L84**（stdout。式で作る）：1 行目 `asr.model.loaded`、2 行目 `asr.warmup.decoded`（`model`、`decode_ms`、`audio_s` を持つ）、3 行目以降 `asr.endpoint.decided`（`id` が 1 から 5、`rule`、`silence_ms`）。
**G84**（L84 に句読点の session を足した物。式で作る）：L84 の 1 行目と 2 行目の間に `gpu.session.created` の行を 2 行足す。1 行は `model` が `punct_bert.onnx`、`actual_provider` と `requested_provider` が `cuda`。もう 1 行は `model` が `silero_vad.onnx`、両方 `cpu`。行の形は窓 E3' の `hayamimi.stdout.log` の実物の行（`event`、`component` `gpu`、`model`、`actual_provider`、`requested_provider`）と同じにする。`asr.warmup.decoded` は `model` `rz`、`decode_ms` 269.864、`audio_s` 10 に `punct_ms` 448.445（JSON の数）を足した行にする。それ以外は L84 と同じ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-84-001 | F84 の `e3q1`。A の `final_latency_ms` を 157 ms（比べる集合の p50）にする。stdout は L84 | 設計の陽性の case。A は p95 の内側 | A ≤ p95 | `Ok`（specified、RP-84 の陽性欄「発話 1 の 1 回目を同じ run の p50 に置いた fixture が通る」） | 陽性 | `a_first_utterance_at_the_p50_of_the_rest_passes` |
| T-RP-84-002 | 001 で A を 209 ms（p95 ちょうど）にする | p95 の線の値 | A ≤ p95 | `Ok`（specified、RP-84 の「p95（nearest-rank）以下」。等号側） | 境界、陽性 | `a_first_utterance_exactly_at_the_p95_passes` |
| T-RP-84-003 | 001 で A を 209.000001 ms にする | p95 の線の 1 桁上（10 の -6 乗ms） | 同上 | `Err` で `e3q1`、A の 209.000001、p95 の 209 を名指す（specified、同上。nearest-rank の 109 番目。補間した p95 は 208.35 付近で、209 を通さない build はここでなく 002 で落ちる） | 境界、陰性 | `a_first_utterance_one_step_over_the_p95_is_rejected` |
| T-RP-84-004 | 001 で A を 209.5 ms にする | A を比べる集合に入れると通ってしまう値 | 比べる集合は A を除く | `Err` で A の 209.5 と p95 の 209 を名指す（derived、A を含めた n = 115 の p95 は順位 110 で 209.5 になり、A ≤ p95 が成り立って通る。「他の」標本の集合から A を除く） | 境界、陰性 | `the_first_utterance_is_not_part_of_the_set_it_is_compared_with` |
| T-RP-84-005 | F84b の `e3q1`。A を 203 ms にする | 有効でない標本を集合に入れない（比べる側が通る） | 有効な標本だけ | `Ok`（specified、RP-84 の「他の有効な標本」。有効な比べる集合は n = 108 で p95 は 203 ms） | 境界、陽性 | `invalid_samples_are_left_out_of_the_set_a_first_utterance_is_compared_with` |
| T-RP-84-006 | 005 で A を 210 ms にする | 同上（落ちる側） | 同上 | `Err` で A の 210 と p95 の 203 を名指す（specified、同上。6 標本を集合に入れた build は p95 が 5000 ms になって通してしまうので、この行で落ちる） | 陰性 | `invalid_samples_do_not_raise_the_limit_a_first_utterance_is_compared_with` |
| T-RP-84-007 | 001 で A の標本の `t_final_us` を null にする（`final` なし） | A が無い | A の `final_latency_ms` が在る | `Err` で `e3q1` と A の `final` が無いことを名指す（implicit、無い A を「p95 以下」と読んで通さない） | 境界、陰性 | `a_first_utterance_without_a_final_is_rejected` |
| T-RP-84-008 | 比べる集合が空の run：A だけが有効で、ほかの 114 標本は `t_final_us` が null | 母数 0 | 比べる集合が在る | `Err` で `e3q1` と比べる標本が無いことを名指す（implicit、空の p95 を作らず、0 件を通さない） | 境界、陰性 | `a_run_with_nothing_to_compare_with_is_rejected` |
| T-RP-84-009 | e-card-form の `rs1` の標本（欄を絞った写し：`utt`、`repeat`、`t_end_us`、`t_final_us`、`final_latency_ms`）を `e3q1` の位置に置く。stdout は L84 | 今の記録。発話 1 の 1 回目が遅い | A ≤ p95 | `Err` で A の 988.338217 と p95 の 213.100368 を名指す（specified、RP-84 の陰性欄「今の記録を読むと、3 run とも発話 1 の 1 回目が p95 を超えて落ちる」。pseudo：`jq -s` で `t_final_us` が在り `d ≥ 120` の標本から A を除いた n = 97、順位 93。2026-10-20 に打った）。stdout は L84 なので、落ちる理由は p95 だけ | 陰性 | `the_recorded_rs1_first_utterance_is_over_the_p95` |
| T-RP-84-010 | 009 の `rs2` と `rs3` | 今の記録の残り 2 run | 同上 | `rs2` は A の 682.495191 が p95 の 220.126125 を超え、`rs3` は A の 807.670646 が p95 の 204.686185 を超えて、どちらも `Err`（pseudo、同上。n はどちらも 97、順位 93） | 陰性 | `the_recorded_rs2_and_rs3_first_utterances_are_over_the_p95` |
| T-RP-84-011 | 009 から 010 の 3 run で、A を比べる集合の p50 に置き換える（`rs1` が 117.679425、`rs2` が 124.455251、`rs3` が 126.553233） | 今の記録の形で A が速い | 同上 | 3 run とも `Ok`（specified、RP-84 の陽性欄を実物の形に当てた。pseudo、同上の `jq`。p50 の順位は 49。009 と 010 は A だけが落ちる理由であることを、この行が示す） | 陽性 | `the_recorded_runs_pass_when_the_first_utterance_is_at_their_p50` |
| T-RP-84-012 | 001 の stdout で `asr.warmup.decoded` の行を `asr.endpoint.decided` の最初の行の直前に置く（L84 そのまま）と、間に別の event を 3 行挟んだ形の 2 入力 | `decoded` が最初の `decided` より前。隣と離れた形 | `decoded` が前 | 2 入力とも `Ok`（specified、RP-84 の「最初の `asr.endpoint.decided` より前の行に在る」） | 境界、陽性 | `a_warmup_decoded_before_the_first_endpoint_decision_passes` |
| T-RP-84-013 | 001 の stdout で `asr.warmup.decoded` の行と最初の `asr.endpoint.decided` の行を入れ替える（`decoded` が 3 行目、最初の `decided` が 2 行目） | 1 行だけ後ろ | `decoded` が前 | `Err` で `e3q1` と `decoded` が最初の `decided` の後ろにあることを名指す（specified、RP-84 の陰性欄「`asr.warmup.decoded` を最初の `asr.endpoint.decided` の後ろに置いた fixture が落ちる」。順が 1 行違うだけの形） | 境界、陰性 | `a_warmup_decoded_after_the_first_endpoint_decision_is_rejected` |
| T-RP-84-014 | 001 の stdout から `asr.warmup.decoded` の行を除く | 0 件 | ちょうど 1 件 | `Err` で `e3q1` と `decoded` が 0 件であることを名指す（specified、RP-84 の「1 件だけ」） | 境界、陰性 | `a_run_without_a_warmup_decoded_line_is_rejected` |
| T-RP-84-015 | 001 の stdout に `asr.warmup.decoded` の行をもう 1 行足す（最初の `decided` の前に 2 件） | 2 件 | ちょうど 1 件 | `Err` で `e3q1` と `decoded` が 2 件であることを名指す（specified、同上。2 回復号しても 1 件に見せない） | 境界、陰性 | `two_warmup_decoded_lines_are_rejected` |
| T-RP-84-016 | 001 の stdout に `asr.warmup.failed` の行を 1 つ足す（`decoded` は 1 件のまま） | 失敗が 1 件 | 0 件 | `Err` で `e3q1` と `failed` の 1 件を名指す。`decoded` の件数は名指さない（specified、RP-84 の「`asr.warmup.failed` が 0 件」。理由を `failed` だけに絞った入力） | 境界、陰性 | `a_warmup_failed_line_is_rejected_on_its_own` |
| T-RP-84-017 | e-card-form の `rs1.hayamimi.stdout.log` の実物（`asr.endpoint.decided` が 141 行、`asr.warmup` を含む行が 0 行） | 今の記録の stdout。数える語の陽性対照 | `decoded` が 1 件 | `Err` で `decoded` が 0 件であることを名指す。同じ判定の中で `asr.endpoint.decided` を 141 件数え、数える語が当たることを示す（pseudo：`grep -c '"asr.endpoint.decided"' rs1.hayamimi.stdout.log` が 141、`grep -c 'asr.warmup'` が 0。`rs2` と `rs3` も 141 と 0。2026-10-20 に打った。0 件と「語を間違えた」を分ける） | 陽性、陰性 | `the_recorded_stdout_has_decisions_and_no_warmup_line` |
| T-RP-84-018 | 2 run の列：`e3q1`（001 の形）と `e3q2`（006 の入力で A が p95 を超える） | 1 本だけが外れる | run ごと | `Err` で `e3q2` を名指す。`e3q1` は名指さない（specified、RP-84 の「各 run」） | 陰性 | `only_the_failing_run_is_named_for_the_first_utterance` |
| T-RP-84-019 | 2 run の列：`e3q1` は A が p95 を超え、`e3q2` は `decoded` が最初の `decided` の後ろ | 2 本とも外れ、理由が別 | run ごと | `Err` で `e3q1` の A と `e3q2` の `decoded` の順の両方を名指す（derived、run ごとに理由を並べ、先に外れた理由で止めない） | 陰性 | `each_failing_run_is_named_with_its_own_reason` |
| T-RP-84-020 | E3'' の run が 0 本の列（名前が `e3q` と数字だけでない `w1`、`w2` だけを渡す） | 空 | E3'' の run が在る | `Err` で E3'' の run が無いことを名指す（implicit、0 本を通さない。T-RP-67-011 と同じ向き） | 境界、陰性 | `a_set_with_no_x_run_is_rejected` |
| T-RP-84-021 | `w1`(名前が `e3q` と数字だけでない)に A が p95 を大きく超える F84 の run を置き、F84 の `e3q1` と一緒に RP-84 に渡す | `e3q` の名前でない run は読まない | E3'' の run だけ | `w1` の A が p95 を大きく超えていても `Ok`（specified、RP-84 の「窓 E3'' の ja の 2 本の run(`e3q<N>`)」。W と X の対は止めた(H132)ので、名前で E3'' の run だけを読む） | 境界、陽性 | `runs_not_named_e3p_are_not_read_for_the_first_utterance` |
| T-RP-84-022 | 001 の stdout の `asr.warmup.decoded` の行から `decode_ms` を除き、`audio_s` を 1 にした物 | 受入の条件に無い欄 | 欄は判定しない | `Ok`（derived、RP-84 の期待値は `decoded` の件数と順と `failed` の件数だけ。`decode_ms` と `audio_s` は判定に入らない。10 秒の無音を復号したことは RP-84 が見ない、3.19 の方針と理由） | 境界 | `warmup_fields_other_than_the_event_name_are_not_judged` |
| T-RP-84-023 | 実走：`docs/results/r8x/` の `e3q<N>.e2e1.jsonl` と `e3q<N>.e2e1.hayamimi.stdout.log` | 実走 | 実走 | 全 E3'' の run(ja の 2 本)で 001 の形（A ≤ p95、`decoded` が 1 件で最初の `decided` の前、`failed` が 0 件）で `Ok`（specified、RP-84）。窓 E3'' の記録が置かれるまでは走らせない | 陽性 | `rp84_the_first_utterance_decodes_as_fast_as_the_rest` |
| T-RP-84-024 | 001 の `e3q1` で A の `d` を 100 ms（120 ms 未満。`t_final_us` は在る）にする。(a) A の `final_latency_ms` を 157 ms にする。(b) A を 210 ms にする | A が有効の線に届かない形 | A は `final` が在れば `d` によらず比べる | (a) は `Ok`、(b) は `Err` で A の 210 と p95 の 209 を名指す（specified、3.19 の「数えた結果」と RP-84。A を「有効でないから比べない」と読む実装は (b) を通してしまうので、(b) で落ちる。比べる集合は A を除く有効な標本の n = 114 のまま） | 境界、陰性 | `a_first_utterance_below_the_validity_line_is_still_compared` |
| T-RP-84-025 | 001 の stdout から `asr.endpoint.decided` の行を全部除く（`asr.model.loaded` と `asr.warmup.decoded` の 2 行だけ） | `decided` が 0 件 | 最初の `decided` が在る | `Err` で `e3q1` と `asr.endpoint.decided` が 0 件であることを名指す（specified、3.19 の「`decided` が 0 件の stdout は落ちる」。「最初の `decided` より前」が空で成り立つ形を通さない。017 の実物が `decided` を 141 件数えることが、数える語の陽性対照） | 境界、陰性 | `a_stdout_without_any_endpoint_decision_is_rejected` |
| T-RP-84-026 | G84 の `e3q1`。A を 157 ms（比べる集合の p50）にする | 句読点の session が在り、`punct_ms` が数 | `punct_ms` が JSON の数 | `Ok`（specified、RP-84 の「`punct_bert.onnx` の `gpu.session.created` が在る stdout では、`asr.warmup.decoded` の `punct_ms` が JSON の数である」の陽性側） | 陽性 | `a_warmup_with_a_numeric_punct_ms_beside_a_punct_session_passes` |
| T-RP-84-027 | 026 の `asr.warmup.decoded` の行から `punct_ms` の欄を除く | 欄の欠け | 同上 | `Err` で `e3q1` と `punct_ms` を名指し、句読点の段を温めていないことを言う（specified、同上の「欠けなら…落ちる」。A は 026 と同じで p95 の内側なので、落ちる理由は `punct_ms` だけ。026 が同じ入力の陽性対照） | 境界、陰性 | `a_warmup_without_punct_ms_beside_a_punct_session_is_rejected` |
| T-RP-84-028 | 026 の `punct_ms` を JSON の `null` にする | 欄は在るが値が無い | 同上 | `Err` で `e3q1` と `punct_ms` を名指す（specified、同上の「null か…」。欄の欠け（027）とは別の形。`null` を数として通す build は、ここで落ちる） | 境界、陰性 | `a_null_punct_ms_beside_a_punct_session_is_rejected` |
| T-RP-84-029 | 026 の `punct_ms` を JSON の文字列 `"448.445"` にする | 型が違う（値は数に読める） | 同上 | `Err` で `e3q1` と `punct_ms` を名指す（specified、同上の「JSON の数」。文字列の数字を数として読む build は、ここで落ちる。T-RP-83-028 と同じ向き） | 境界、陰性 | `a_punct_ms_written_as_a_string_is_rejected` |
| T-RP-84-030 | 026 の `punct_ms` を JSON の数 `0` にする | 数の下側。数であり、`null` でない | 同上 | `Ok`（derived、判定は「数であること」だけで、値の大きさは見ない。0 を欠けと読んで落とす build は、ここで落ちる。温めの時間の下限は RP-84 の期待値に無い） | 境界、陽性 | `a_punct_ms_of_zero_is_a_number_and_passes` |
| T-RP-84-031 | 2 入力。(a) 026 から `model` が `punct_bert.onnx` の `gpu.session.created` の行を除き（`silero_vad.onnx` の 1 行だけが残る）、`punct_ms` を JSON の `null` にする。(b) 026 の `punct_bert.onnx` の行の `model` を `punct_bert_int8.onnx` にし、`punct_ms` を欄ごと除く | 検出器の陰性対照。句読点の session が無い stdout と、名前の違う session | `model` が `punct_bert.onnx` に等しい行が在る時だけ、`punct_ms` を求める | どちらも `Ok`（specified、同上の「`punct_bert.onnx` の `gpu.session.created` が在る stdout では」。条件は `punct_bert.onnx` に等しい `model`。001 の L84 も session の行が無く `punct_ms` が無いまま `Ok` で、(a) と合わせて、句読点の session が無い stdout に `punct_ms` を求めないことを固定する。(b) は部分一致で session を見つける build を落とす。026 から 029 が検出器の働く側の対照） | 境界、陰性 | `punct_ms_is_not_required_when_no_punct_bert_session_is_logged` |
| T-RP-84-032 | 窓 E3' の `e3p1.e2e1` の写しを、名前 `e3q1` の位置に置く。標本は欄を絞った写し（`utt`、`repeat`、`t_end_us`、`t_final_us`、`final_latency_ms`）、stdout は `e3p1.e2e1.hayamimi.stdout.log` の実物の全部 | 窓 E3' の記録を、E3'' の名前で読ませる | A ≤ p95 かつ `punct_ms` が数 | `Err` で `e3q1`、A の 591.784852、p95 の 191.265735、`punct_ms` の 4 つを名指す（specified、RP-84 の陰性欄「窓 E3' の `e3p1` と `e3p2` を `e3q` の名前で読ませた fixture も、発話 1 の 1 回目が p95 を超え、`punct_ms` が欠けて落ちる」。理由は 2 つで、先に外れた理由で止めず並べる。pseudo：`git show 7b4b017:docs/results/r8x/e3p1.e2e1.jsonl` を `jq -s` で読み、`t_final_us` が在り `d ≥ 120` の標本から A を除いた n = 97、nearest-rank の p95 が 191.265735。stdout は `gpu.session.created` が 6 行で `model` が `punct_bert.onnx` の行が 1 行、`asr.warmup.decoded` が 1 行で `punct_ms` の欄が無い。2026-10-21 に打った） | 陰性 | `the_recorded_first_window_run_one_read_under_the_new_name_fails_on_both_counts` |
| T-RP-84-033 | 032 の `e3p2.e2e1` の写し（名前 `e3q2` の位置） | 窓 E3' の 2 本目 | 同上 | `Err` で `e3q2`、A の 659.042691、p95 の 229.75141、`punct_ms` を名指す（specified、同上。pseudo：032 と同じ `jq` で n = 97、p95 が 229.75141。stdout の `model` が `punct_bert.onnx` の行は 1 行、`punct_ms` の欄が無い。2026-10-21 に打った） | 陰性 | `the_recorded_first_window_run_two_read_under_the_new_name_fails_on_both_counts` |
| T-RP-84-034 | 2 run の列：名前が `e3p1` と `e3p2` で、どちらも 026 の内容（G84、A が p95 の内側、`punct_ms` が数） | 窓 E3' の名前だけの列。内容は通る形 | E3'' の run が在る | `Err` で E3'' の run が無いことを名指す（specified、3.21 の 4「`e3p` の記録は記録として残り、test は読まない」。020 と同じ型の 0 本。名前を替えただけの test は、旧名が通ってもここで落ちない。`e3p1` と `e3p2` の A や `punct_ms` は名指さない） | 境界、陰性 | `a_set_of_runs_all_named_for_the_old_window_is_rejected` |
| T-RP-84-035 | 3 run の列：`e3p1`（G84 で A を 5000 ms にし、`punct_ms` の欄を除く。通らない内容）、`e3q1`（026）、`e3q2`（026） | 旧名の run は読まれない | E3'' の run だけ | `Ok`。`e3p1` を名指さない（specified、同上。021 の `w1` と同じ向きで、`e3p` の名前を別の理由で読む build は、`e3p1` の A と `punct_ms` で落ちる。025 までの `e3p1` の行は `e3q1` に直したので、旧名が通る build を落とす行はこの 2 行（034、035）だけ） | 境界、陽性 | `a_run_named_for_the_old_window_is_not_read_beside_valid_runs` |
| T-RP-84-036 | 4 run の列：名前が `e3q1x`、`e3q_1`、`E3Q1`、`xe3q1`。どれも 026 の内容 | 名前の境界。`e3q` と数字だけでない物 | E3'' の run が在る | `Err` で E3'' の run が無いことを名指す（derived、設計の「読む run の名前を `e3p` から `e3q` に替える」と 25 行までの `e3q` と数字だけという読み。前に付く字、後ろに付く字、大文字、`_` を通さない） | 境界、陰性 | `names_other_than_e3q_and_digits_are_not_read` |
| T-RP-84-037 | 1 run の列：名前が `e3q12`（数字が 2 桁）で、G84 の A を 210 ms（p95 の 209 を超える）にした物 | 名前の境界の内側 | `e3q` と数字だけは読む | `Err` で `e3q12`、A の 210、p95 の 209 を名指す（derived、同上。数字が 1 桁だけを読む build は `e3q12` を読まず、0 本の `Err` になるので、`e3q12` と A を名指すこの行で落ちる） | 境界、陰性 | `a_run_with_a_two_digit_e3q_name_is_read` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 同じ run の p50 の集合。設計は「同じ run の p50」と書く。011 は比べる集合（A を除く有効な標本）の p50 を採った。
- 009 から 011 の fixture は、e-card-form の標本を欄を絞って `native/hayamimi-eval/tests/fixtures/` に写す。写しの元は git の外（利用者の home の `.cache/hayamimi-e2e-20261019/e-card-form/`）で、test は元を読まない。
- 初回の復号が 10 秒の無音であること（`audio_s` が 10）と、`decode_ms` の値は、RP-84 の期待値に無い。unit test の持ち主は R8x-3 の席だが、その受入 ID は無い。

### 4.77 RP-85（数字の並びと `。` `、` の有無だけが違う速報の訳の使い回し）

**001 から 033 は `native/hayamimi-relay/tests/rp85.rs` の 33 本と 1 対 1 で実装済み**（持ち主 R8x-4）。**(4) の数え方を 3.21 の 2 に直した行（026 から 031 の入力と期待値、足した 034 から 052）は、実装が追いついていない。** R8x-8 の席が `rp85.rs` を直すまで通らない。026 から 031 の関数の末尾は変えない（実装との 1 対 1 を保つため）。031 の行は、窓 E3'' の記録が置かれるまで走らせない。025 は窓の席が起こした 18084 の llama-server に対して打つ。
鍵の読み：広げた鍵が除くのは、文の中の `。` `、`、数字の並び（1 つの印へ）、空白である。文末の印（`。` `、` `？` `！` の並び）は除かない。T-RP-69-006 の「`X。。` と `X、` は T をもう一度頼む」が残るためで、001 から 005 の対はこの読みで合う。
test：`native/hayamimi-relay/tests/rp85.rs`。受入 test の名前は `rp85_a_draft_differing_only_in_numerals_or_inner_marks_is_reused_and_checked_by_v`。実走の行（T-RP-85-031）と probe の行（T-RP-85-025）だけが `#[ignore]` を持つ。probe の行は 18084 の llama-server を使い、実走の行は窓 E3'' の記録を読む。ほかの行は偽の認識器と偽の llama-server を使い、`--ignored` を付けずに通る。
command：台本の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp85`。実走の行はこれに `-- --ignored` を足す。probe の行は build 済みの test binary を直に起こし、cargo の親 process を残さない（起こす順は D の RP-85 の行の (a) から (f)）。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない（031 だけは受入 test の名前の全部）。引数の形は実装の席が決める。
設計は D 3.19 の「使い回しの鍵の広げ」と、D 3.18 の 1 から 5、D 6 と 7 の RP-85、D 10 の H117 である。

**F85**（偽の認識器と偽の llama-server。`tests/support/events.rs` の `FakeRecognizer`、`run_relay_partials`、`FakeLlm` と F69 と同じ形）：

- **流れ**：F69 と同じ。先頭の確定文 L0（`mic-1`）の訳が出るまで待ち、対象の発話は次の `mic-2`。速報（`id` を持たない）の text を d、確定文の text を f と呼ぶ。行は `lang` が `ja` で、en の行だけ `en`。
- **偽の server**：T の依頼に 300 ms 後、E は 100 ms 後に答える。T の答えは既定で原文の字写し（F69 と同じ）で、台本の答えを置く行は行の中に書く。
- **実際の組**：d と f は e-card-form の記録（`rs1`〜`rs3` の events）から抜いた物で、図の `…` は実物では省かずに書く。**P7**（発話 7）：d は `千九百四十年八月十五日連合国は南フランスに侵攻しその侵攻はドラグーン作戦と呼ばれました`、f は `1940年八月15日連合国は南フランスに侵攻しその侵攻はドラグーン作戦と呼ばれました。`。**P11**（発話 11）：d は `そこの上流から丸太を組んでいかだにしてどんどん流したんですこれを`、f は `そこの上流から丸太を組んでいかだにしてどんどん流したんです。これを。`。**P20**（発話 20）：d は `言ったろヒーローよりもてめぇを信じる`、f は `言ったろ。ヒーローよりもてめぇを信じる。`。
- **数え方**：「T の依頼の数」は、種類が T で、原文が指した文字列に等しい依頼の数（F69 と同じ）。`translate.draft.reused` は `id`、`source`、`match`、`state`、`waited_ms`、`verdict` の 6 項目を持つ（3.18 の 5。`match` に `normalized` が足される）。

**F85q**（026 から 031 と 034 から 052 の fixture。式で作る。実測のふりをしない）：窓 E3'' の ja の 2 run `e3q1` と `e3q2`。置く物は run ごとに `<run>.e2e1.jsonl`（標本）、`<run>.e2e1.events.jsonl`（認識器の event）、`<run>.e2e1.relay.stdout.log`（中継の stdout）の 3 つである。

- **標本**：発話 7 の 5 回（標本 1 から 5。結ぶ `final` の `id` は `mic-71` から `mic-75`）。標本の行は `utt` 7、`repeat` 1 から 5、`t_final_us`、`t_tr_us` を持ち、`id` の欄は持たない（`hayamimi-eval stream` の行と同じ）。判定は標本を `utt 7 repeat <N>` で名指し、結べた標本には結んだ `final` の `id`（`mic-7k`）を足し、結べない標本には `t_final_us` を足す。訳の段は `(t_tr_us − t_final_us)/1000`。`L` の欄は式で埋め、この判定は読まない。発話 7 以外の標本は置かない。
- **events**：標本ごとに、`t_us` が `t_final_us` に等しく `id` を持つ `final`（確定文 f）と、その前の最後の速報（`id` を持たない。text が d）。速報は、1 つ前の `final` の `t_us` より後で、この `final` の `t_us` より前の `partial` のうち最後の物（run の最初の `final` は run の頭から数える。3.21 の 2 の 1）。標本 k の速報は、標本 k − 1 の `final` の後に置く。標本と確定文は `t_final_us` と `t_us` で結ぶ（3.19 の「置き換えの式」と同じ）。
- **鍵が合う標本**：d が P7 の d、f が P7 の f。製品の `hayamimi_relay::partial::reuse_match`（`japanese` は真）が `normalized` を返す（001 の組）。訳の段は 120000 µs。中継の stdout に、同じ `id` の `translate.draft.reused`（`match` が `normalized`）が 1 件ある。
- **鍵が合わない標本**：d が P7 の d、f が P7 の f の `連合国は` を `連合国が` にした文。同じ関数が `None` を返す（011 の組）。訳の段は 418540 µs。`translate.draft.reused` を持たない。
- **既定**：標本 1、2、3、5 が鍵が合い、標本 4 が合わない（窓 E3' の発話 7 と同じ形。設計の「直した数え方で通る側」の形）。各 run の中継の stdout は、発話 7 以外の `id`（`mic-31`）の `normalized` の `translate.draft.reused` を 1 件持つ。したがって鍵が合う標本の reuse を消しても、run の `normalized` の件数は 0 にならない。
- **別 `id` の対照**（陰性の行の共通）：名指しを見る陰性の行（027、029、035、037、038、039、041、046、047、049、051）は、同じ test の中で、全標本の行に `id` の欄 `mic-99` を書き込んだ入力も打つ。名指しは書き込む前と同じで、`mic-99` は出力に現れない（標本の行の `id` は読まない）。
- **出力**：判定は、合わない標本を run の名前、`utt 7 repeat <N>`、結んだ `final` の `id`、確定文、最後の速報、訳の段、理由（`key` か `no_draft`）つきで出力に並べる。理由 `key` は、鍵の関数が `None` を返した標本、`no_draft` は区切りの中に速報が無い標本である。落ちる時の出力にも並べる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-85-001 | F85。P7 の d を速報で送り、速報の訳（台本の答え `On August 15, 1940, the Allies invaded southern France, and the invasion was called Operation Dragoon.`。この答えは変えない）が出るのを待つ。そのあとに f を `final`（`mic-2`）で送る | 数字の書き方だけが違い、f が `。` で終わる（設計の合う側の 1 組め） | 再使用 | 原文が f の T の依頼は 0 本、原文が d の T の依頼は 1 本。`translate.draft.reused` が 1 件で `match` が `normalized`、`verdict` が `ok`。`final` の訳は `of` が `final`、`stage` が `t`、`settled` が偽、`src_sha256` が f の sha256（specified、RP-85 の (1) と 3.19 の「使い回しの鍵の広げ」。台本の答えは実物の訳の形のままで、V の通りやすい形に変えて通さない。台本の答えが V を通ることは、V を直接呼ぶ 032 が先に確かめる） | 陽性 | `a_draft_with_other_numerals_is_reused_under_the_widened_key` |
| T-RP-85-002 | F85。P11 の d を速報、f を `final` で送る（答えは字写し） | 句点が文の中に入り、f が `。` で終わる（設計の合う側の 2 組め） | 再使用 | 001 の形。`match` が `normalized`、`verdict` が `ok`、原文が f の T の依頼は 0 本（specified、RP-85 の (1)） | 陽性 | `a_draft_without_inner_marks_is_reused_under_the_widened_key` |
| T-RP-85-003 | F85。P20 の d と f | 句点が文の最初の節の後と末尾に在る | 再使用 | 001 の形。`match` が `normalized`（specified、同上。(1) の 2 組め以外の同値類。f の文の中の `。` が d に無い） | 陽性 | `a_draft_without_the_inner_mark_after_the_first_clause_is_reused` |
| T-RP-85-004 | F85。d が `ところで、昨日は雨でした`、f が `ところで昨日は雨でした。` | `、` だけが違う | 再使用 | 001 の形。`match` が `normalized`（specified、3.19 の「文の中の `。` と `、` を除き、文末の印は残した物が同じ」。`、` の側の同値類） | 陽性 | `a_draft_with_a_comma_the_final_lacks_is_reused` |
| T-RP-85-005 | F85。d が `被害は三件でした`、f が `被害は3件でした`（`。` を足さない） | 数字の書き方だけが違い、印は同じ | 再使用 | 001 の形。`match` が `normalized`（specified、3.19 の「数字の並びをそれぞれ 1 つの印に置き換え」。印の有無に頼らない側） | 境界、陽性 | `a_draft_differing_only_in_numerals_is_reused` |
| T-RP-85-006 | F85。d と f が同じ文字列（`今日は晴れです`） | 今の鍵の `same`。広げた鍵より先 | `same` | `match` が `same`（`normalized` でない）。T の依頼は d の 1 本だけ（specified、RP-85 の陽性欄「RP-69 の今の case（`same` と `closing_mark`）が同じ test の binary で通る」。T-RP-69-003 と同じ入力） | 陽性 | `a_final_with_the_same_text_still_logs_same` |
| T-RP-85-007 | F85。d が `今日は晴れです`、f が `今日は晴れです。` | 今の鍵の `closing_mark` | `closing_mark` | `match` が `closing_mark`（`normalized` でない）。T の依頼は d の 1 本だけ（specified、同上。T-RP-69-001 と同じ入力） | 陽性 | `a_final_that_only_adds_the_closing_mark_still_logs_closing_mark` |
| T-RP-85-008 | F85。d が `ところで、昨日は雨でした`、f が `ところで、昨日は雨でした。` | 今の鍵にも広げた鍵にも合う。優先の確かめ | 今の鍵が先 | `match` が `closing_mark`（`normalized` でない）（derived、3.18 の 1 の鍵が先に合う物は値を変えない。3.19 は `closing_mark` の 2 つの値をそのままと書く） | 境界 | `a_pair_both_keys_match_logs_the_existing_value` |
| T-RP-85-009 | F85。d が `これは本当ですか`、f が `これは本当ですか？` | `？` を足した形 | 再使用しない | `translate.draft.reused` が 0 件。原文が f の T の依頼が 1 本出る（specified、RP-85 の (1)「…ですか」と「…ですか？」は合わない。`？` は除かない、3.19。T-RP-69-004 と同じ向き） | 陰性 | `a_question_mark_added_by_the_final_keeps_the_pair_apart` |
| T-RP-85-010 | F85。d が `すごいですね`、f が `すごいですね！` | `！` を足した形 | 再使用しない | 009 と同じ（specified、同上。`！` も除かない） | 陰性 | `an_exclamation_mark_added_by_the_final_keeps_the_pair_apart` |
| T-RP-85-011 | F85。P7 の d の `連合国は` を `連合国が` にした d と、P7 の f | 文字の違う対（数字と印は合う） | 再使用しない | `translate.draft.reused` が 0 件。原文が f の T の依頼が 1 本（specified、RP-85 の (1)「文字の違う対（「連合国は」と「連合国が」）」） | 陰性 | `a_draft_with_a_different_letter_is_not_reused` |
| T-RP-85-012 | F85。d が `三件と五件です`、f が `3件と5件と7件です。` | 数字の並びの数が違う（印に直すと `#件と#件です` と `#件と#件と#件です`） | 再使用しない | `translate.draft.reused` が 0 件。T の依頼が f で 1 本（derived、3.19「数字の並びをそれぞれ 1 つの印に置き換え」。並びの数が違う対を、数字を全部 1 つにまとめる build が合わせてしまわない） | 境界、陰性 | `a_pair_with_a_different_number_of_numerals_is_not_reused` |
| T-RP-85-013 | F85（en）。(a) d が `In 1944 the allies landed`、f が `In 1940 the allies landed`。(b) d が `well we left`、f が `well, we left.` | en の行。数字だけ、印だけが違う | 再使用しない | 2 入力とも `translate.draft.reused` が 0 件で、T の依頼が f で 1 本（specified、RP-85 の (1)「en の行は合わない」。3.19「en の行には掛けない」。(a) は ja なら `normalized` で合う形で、en で合わないことを示す） | 陰性 | `an_english_line_is_never_matched_by_the_widened_key` |
| T-RP-85-014 | F85。001 の f に、速報の `id` と別の `id`（`mic-3`）を付ける | 鍵の `id` が違う | 再使用しない | `translate.draft.reused` が 0 件（specified、3.18 の 1「音源、`id`、文字列の 3 つが合う時だけ」。広げた鍵が `id` の検査を外さない） | 陰性 | `the_widened_key_still_needs_the_same_id` |
| T-RP-85-015 | F85。001 の f の `lang` を `en` にして送る（文字列は ja のまま） | 鍵が `lang` を見る | 再使用しない | `translate.draft.reused` が 0 件（specified、3.19「ja の行」。文字列でなく `lang` で広げた鍵を掛ける） | 境界、陰性 | `the_widened_key_follows_the_lang_of_the_final` |
| T-RP-85-016 | F85。d が `被害は三件でした`（台本の答え `There were three cases of damage`）、f が `被害は4件でした。` | 数字の書き方が違い、数の値が違う（設計の (2)）。V が数で止める | V が止める | `translate.draft.reused` が 1 件で `match` が `normalized`、`state` が `done`、`verdict` が `held`。`translate.draft.held` が 1 件。E の依頼が 1 本で、`final` から T-RP-69-018 と同じ上限（`PROMPT`）以内に出る。先の訳（`of` が `final`、`stage` が `t`）は 0 件。そのあと E の訳（`settled` が真）が 1 件出る（specified、RP-85 の (2)。V の原文は f にする、3.18 の 4。V の原文が d だと「三件」の 3 が答えの `three` と合って通ってしまうので、この行で落ちる） | 陰性 | `a_reused_draft_with_another_number_is_stopped_by_v_and_e_is_asked` |
| T-RP-85-017 | 016 で f を `被害は3件でした。` にする | 数の値が合う。016 の陽性対照 | V が通す | `translate.draft.reused` が 1 件、`match` が `normalized`、`verdict` が `ok`。`final` の訳は `of` が `final`、`stage` が `t`。E の依頼は T の後に 1 本（specified、同上。V が数字の書き方の違いだけでは止めないことを示し、016 の止まる理由が数の値であることを固定する） | 陽性 | `a_reused_draft_with_the_same_number_passes_v` |
| T-RP-85-018 | `docs/results/r8x/rp85_pairs.jsonl`（作る）の実物 | 品質の probe の対の組。広げた鍵だけに合う対 | 対の数 5 | 5 行で、どの行も d と f が広げた鍵で合い、`same` と `closing_mark` に合わず、同じ対が 2 行に在らない。対は P7、P11、P20 と、`この町はウポル島にあり人口は四万人弱です` と `この町はウポル島にあり人口は4万人弱です。`、`一分間で沸騰する地域もあれば沸騰するまでに何分もかかる地域もあります` と `一分間で沸騰する地域もあれば。沸騰するまでに何分もかかる地域もあります。` の 5 つ（specified、RP-85 の probe の対の組「広げた鍵だけに合う最後の速報と確定文の対を重ねずに抜いた物」。pseudo：e-card-form の `rs1`〜`rs3` を 3.19 の置き換えの式の `wide` かつ `same` でない物、で絞った (`utt`、確定文、速報) の異なる組が 5。2026-10-20 に打った。発話は 2、5、7、11、20） | 陽性 | `the_pairs_file_holds_only_pairs_the_widened_key_alone_matches` |
| T-RP-85-019 | 018 の fixture に、`closing_mark` で合う対（`これは雨です` と `これは雨です。`）を 1 行足した物 | 広げた鍵だけに合う物でない対 | 広げた鍵だけ | `Err` でその行を名指す（specified、RP-85 の probe の対の組。今の鍵で合う対を入れると、広げた鍵の効き目を測れない） | 陰性 | `a_pair_the_existing_key_matches_is_rejected_from_the_pairs_file` |
| T-RP-85-020 | 018 の fixture に、同じ対を 2 行にした物 | 重なり | 重ねない | `Err` で重なった対を名指す（specified、同上「重ねずに」） | 境界、陰性 | `a_pair_listed_twice_is_rejected_from_the_pairs_file` |
| T-RP-85-021 | 対の組の file が無い | file の欠け | file が在る | `Err` で path を名指す（implicit、空の組を「止まる行 0 ≤ 0」と読んで通さない。T の 4.1 の「結果の file が無い時は path を名指す」と同じ） | 陰性 | `a_missing_pairs_file_is_rejected_by_path` |
| T-RP-85-022 | 止まる行の数の比べ：(d の答えで止まる数、f の答えで止まる数) が (0, 0)、(2, 2)、(1, 2)、(3, 2)、(1, 0) の 5 入力 | 等号と両側 | d の側 ≤ f の側 | (0, 0)、(2, 2)、(1, 2) は `Ok`。(3, 2) と (1, 0) は `Err` で 2 つの数を名指す（specified、RP-85 の (3)「確定文の T の答えに V を掛けた時以下」。等号は通す） | 境界 | `the_draft_stop_count_may_equal_but_not_exceed_the_final_stop_count` |
| T-RP-85-023 | 数える側の対照：対（d が `被害は三件でした`、f が `被害は4件でした。`）。(a) 速報の T の答えを `There were three cases of damage`、確定文の T の答えを `There were 4 cases of damage` にする。(b) (a) の速報の T の答えを `There were 4 cases of damage` にする | V が止まる行を数える | 数は答えで決まる | (a) は d の側が 1、f の側が 0 で、比べる関数が `Err`。(b) は 0 と 0 で `Ok`（derived、V の数の検査が止める行を数える側が、止まった行を 0 と読まない。数える関数の陽性対照と陰性対照） | 陽性、陰性 | `the_stop_counter_counts_a_draft_answer_with_another_number` |
| T-RP-85-024 | 品質の probe の対の数 0 | 母数 0 | 対が在る | `Err` で対が無いことを名指す（implicit、0 件の比べ「0 ≤ 0」を通さない） | 境界、陰性 | `a_probe_over_no_pairs_is_rejected` |
| T-RP-85-025 | probe（窓の席が起こす 18084 の llama-server）：018 の 5 対で、速報の文の T の答えに f を原文として V を掛けた時に止まる行の数 a と、f の T の答えに V を掛けた時に止まる行の数 b を数える | 実走（GPU の llama-server） | a ≤ b | `Ok`。a と b の値を出力に書く（specified、RP-85 の (3)。値は実走で決まる。a ≤ b だけを判定する）。18084 の llama-server が答えない時は、`Err` で接続の誤りを名指し、a ≤ b と読まない | 陽性 | `the_draft_answers_stop_no_more_often_than_the_final_answers` |
| T-RP-85-026 | F85q の既定の 2 run（`e3q1`、`e3q2`）。ただし各 run の標本 3（鍵が合う）の訳の段を 182000 µs にする | 鍵が合う標本の訳の段の最大が線ちょうど。鍵が合う標本は 4（最小の 3 より 1 多い）。合わない標本が 1 | 鍵が合う標本の最大 ≤ 182 ms、鍵が合う標本 ≥ 3、各 run に `normalized` の reuse が 1 件以上 | `Ok`。出力に、各 run の合わない標本 1 件（標本 4、`utt 7 repeat 4`、`mic-74`、訳の段 418.540 ms、確定文と最後の速報、理由 `key`）が並ぶ（specified、RP-85 の (4) と 3.21 の 2。鍵が合う標本の最大は 182000 µs で 182 ms ちょうど。合わない標本の 418540 µs は 182 ms の判定に入らない。標本が 4 なので、nearest-rank の p95 は最大と同じで、名前を p95 と書かず最大と書く） | 境界、陽性 | `x_runs_with_a_reuse_each_and_utterance_7_at_182_ms_pass` |
| T-RP-85-027 | 026 の `e3q1` の標本 3 を 182001 µs にする（`e3q2` は 182000 µs のまま） | 線の 1 µs 上 | 同上 | `Err` で `e3q1`、標本 3（`utt 7 repeat 3`、`mic-73`）、訳の段の最大 182001 µs と 182 ms を名指す。`e3q2` は名指さない。別 `id` の対照でも同じ名指し（specified、同上。最小の桁 1 つ分だけ外れた値。026 が同じ入力の 1 µs 下の陽性対照） | 境界、陰性 | `utterance_7_one_microsecond_over_182_ms_is_rejected` |
| T-RP-85-028 | 026 で `e3q2` の `translate.draft.reused` 5 件（標本 1、2、3、5 の 4 件と `mic-31` の 1 件）の `match` を `same` にする（`id` と件数は変えない） | `normalized` が 0 件で、鍵が合う標本の reuse は `id` で在る | 各 run に `normalized` が 1 件以上 | `Err` で `e3q2` と `normalized` が 0 件であることを名指す。鍵が合う標本は名指さない（specified、RP-85 の (4)「`match` が `normalized`」。`same` と `closing_mark` の件は数えない。数える語が当たることは `e3q1` の 5 件が示す） | 境界、陰性 | `a_run_without_a_normalized_reuse_is_rejected` |
| T-RP-85-029 | F85q の既定の 2 run から、中継の stdout の `translate.draft.reused` を全部除く（`mic-31` の件も除く） | 使い回しが 1 件も無い run。鍵が合う標本の 4 つが reuse を持たない | 各 run に `normalized` が 1 件以上。鍵が合う標本は reuse を持つ | `Err` で `e3q1` と `e3q2` の両方について、`normalized` が 0 件であることと、鍵が合う標本 1、2、3、5（`utt 7 repeat 1`、`2`、`3`、`5` と `mic-71`、`mic-72`、`mic-73`、`mic-75`）が `translate.draft.reused` を持たないことを名指す。合わない標本 4 は reuse の欠けとして名指さない。別 `id` の対照でも同じ名指し（specified、RP-85 の (4) と 3.21 の 2 の 2。run ごとに理由を並べる。関数の末尾は実装との 1 対 1 のため、旧い記録を読んだ時の名前のまま） | 陰性 | `the_recorded_utterance_7_is_over_182_ms_without_a_reuse` |
| T-RP-85-030 | F85q の既定で、`e3q1` の発話 7 の 5 標本の `t_tr_us` を全部 `null` にする（訳の取れた標本が 0 件。`e3q2` は既定のまま） | 母数 0 | 標本が在る | `Err` で `e3q1` と発話 7 の標本が無いことを名指す。`e3q2` は名指さない（implicit、空の最大と、鍵が合う標本 0 の「3 以上」を通さない） | 境界、陰性 | `an_x_run_without_utterance_7_samples_is_rejected` |
| T-RP-85-031 | 実走：`docs/results/r8x/` の E3'' の ja の run（`e3q<N>.e2e1.jsonl`）と、その run の中継の stdout（`e3q<N>.e2e1.relay.stdout.log`） | 実走 | 実走 | 026 の形（各 run に `normalized` の reuse が 1 件以上、鍵が合う標本が 3 以上でどれも同じ `id` の reuse を持ち、その訳の段の最大が 182 ms 以下）で `Ok`。合わない標本の数と訳の段が出力に並ぶ（specified、RP-85 の (4)。標本の分けは製品の `reuse_match` で打つ）。窓 E3'' の記録が置かれるまでは走らせない | 陽性 | `rp85_a_draft_differing_only_in_numerals_or_inner_marks_is_reused_and_checked_by_v` |
| T-RP-85-032 | **(0) V を直接呼ぶ**：原文 `1940年八月15日連合国は南フランスに侵攻しその侵攻はドラグーン作戦と呼ばれました。`（P7 の f）と、答え `On August 15, 1940, the Allies invaded southern France, and the invasion was called Operation Dragoon.` を V に渡す。偽の認識器も偽の server も使わない | 実物の訳の形。日付が `八月十五日` と `August 15` で書き方が違う | V が通す | V の判定が通す（`verdict` が `ok` の側）（specified、RP-85 の (0)。001 の台本の答えと同じ文字列。通らない時は、R8x-4 は鍵を広げる変更を足さずに止まり、評議へ上げる。答えを V の通る形に変えて 001 を通さない。V が止める側の対照は 016 が持つ。この行は通る側だけを固定する） | 陽性 | `v_passes_the_scripted_answer_for_utterance_7` |
| T-RP-85-033 | 026 で `e3q1` の中継の stdout に `log.queue.dropped` の行を 1 行足す | log の取りこぼし | その run を無効にする | `Err` で `e3q1` と `log.queue.dropped` を名指す（specified、D 7 の「`log.queue.dropped` が 1 行でもあればその run を無効として名指す」） | 陰性 | `a_run_whose_log_dropped_lines_is_rejected` |
| T-RP-85-034 | F85q の `e3q1` で、標本 2 を合わない文にする（標本 2 の `translate.draft.reused` を除き、訳の段を 300000 µs にする）。鍵が合う標本は 1、3、5 の 3。`e3q2` は既定 | 鍵が合う標本の数の線ちょうど（3） | 鍵が合う標本 ≥ 3 | `Ok`。出力に `e3q1` の合わない標本 2 件（`utt 7 repeat 2`、`4`）と `e3q2` の 1 件（`utt 7 repeat 4`）が並び、結んだ `final` の `id` は順に `mic-72`、`mic-74`、`mic-74` で、どれも理由は `key`（specified、RP-85 の (4)「各 run で 3 以上（5 回の過半）」。合わない標本の訳の段 300000 µs と 418540 µs は 182 ms の判定に入らない。035 が 1 つ下の陰性対照） | 境界、陽性 | `three_keyed_samples_are_enough_and_the_unmatched_ones_are_listed` |
| T-RP-85-035 | 034 で `e3q1` の標本 3 も合わない文にする（標本 3 の reuse を除き、訳の段を 300000 µs にする）。鍵が合う標本は 1、5 の 2 | 鍵が合う標本の数の線の 1 つ下（2） | 同上 | `Err` で `e3q1`、鍵が合う標本が 2 であること、「3 以上」を名指し、合わない標本 3 件（`utt 7 repeat 2`、`3`、`4` と `mic-72`、`mic-73`、`mic-74`）の確定文と最後の速報を並べる。`e3q2` は名指さない。別 `id` の対照でも同じ名指し（specified、同上「3 未満の run は、合わなかった標本の確定文と速報を並べて落ちる」。鍵が合う標本の訳の段は 120000 µs で 182 ms の内側なので、落ちる理由は数だけ。034 が同じ形の 1 つ上の陽性対照） | 境界、陰性 | `two_keyed_samples_are_too_few_and_the_unmatched_ones_are_listed` |
| T-RP-85-036 | 026 の `e3q1` で、標本 4 の f を P7 の f にして（鍵が合う文）、`mic-74` の `normalized` の reuse を 1 件足し、訳の段を 120000 µs にする。鍵が合う標本は 5。`e3q2` は既定 | 鍵が合う標本の数の上側（全部）。合わない標本が 0 | 同上。最大 ≤ 182 ms | `Ok`。出力の `e3q1` の合わない標本は 0 件、`e3q2` は 1 件（specified、同上。合わない標本が 0 件でも落ちず、0 件と出る。標本 4 が鍵の合う側へ動くことは、f の字だけで決まる） | 境界、陽性 | `five_keyed_samples_pass_and_none_is_listed_as_unmatched` |
| T-RP-85-037 | 026 の `e3q1` の 5 標本とも合わない文にする（標本 1、2、3、5 の reuse を除き、f の `連合国は` を `連合国が` にする。`mic-31` の `normalized` の reuse は残す）。`e3q2` は既定 | 鍵が合う標本が 0。標本は 5 在る（030 の標本無しとは別の形） | 鍵が合う標本 ≥ 3 | `Err` で `e3q1`、鍵が合う標本が 0 であること、5 標本（`utt 7 repeat 1` から `5`、`mic-71` から `mic-75`）の確定文と最後の速報を名指す。`normalized` の件数（`mic-31` の 1 件）では落ちない。`e3q2` は名指さない。別 `id` の対照でも同じ名指し（specified、同上） | 境界、陰性 | `no_keyed_sample_is_rejected_with_all_five_listed` |
| T-RP-85-038 | 026 の `e3q1` で、標本 1 の `translate.draft.reused`（`mic-71`）を除く（`e3q1` に `normalized` の reuse は 4 件残る）。`e3q2` は既定 | 鍵が合うのに使い回さない標本 | 鍵が合う標本は同じ `id` の reuse を持つ | `Err` で `e3q1` と標本 1（`utt 7 repeat 1`、`mic-71`）を名指し、`translate.draft.reused` が無いことを言う。標本 2、3、5 は名指さない。別 `id` の対照でも同じ名指しで、標本の行の `mic-99` は出ない（specified、3.21 の 2 の 2「鍵が合うのに使い回さなかった中継の欠けを捕まえる」。標本 1 の訳の段は 120000 µs で 182 ms の内側なので、落ちる理由は reuse の欠けだけ。026 が同じ形の陽性対照） | 陰性 | `a_keyed_sample_without_a_reuse_is_named_and_rejected` |
| T-RP-85-039 | 038 で reuse を除く代わりに、標本 1 の reuse の `id` を `mic-91`（どの標本にも無い `id`）に書き換える（`translate.draft.reused` の件数は変わらない） | reuse は在るが別の `id` | 同じ `id` | `Err` で `e3q1` と標本 1（`utt 7 repeat 1`、`mic-71`）を名指す。別 `id` の対照でも同じ名指し（derived、結びの鍵は `id`。reuse の件数や `normalized` の有無だけを数える build は、件数が変わらないので通してしまい、ここで落ちる） | 境界、陰性 | `a_reuse_line_under_another_id_does_not_stand_for_the_keyed_sample` |
| T-RP-85-040 | 026 の `e3q1` で、合わない標本 4 の訳の段を 1000000 µs にする | 合わない標本が遅い | 182 ms の判定に入らない | `Ok`。出力に標本 4 と訳の段 1000.000 ms と理由 `key` が並ぶ（specified、3.21 の 2 の 4「数と訳の段を出力に並べ、182 ms の判定には入れない」。合わない標本を判定に入れる build は、ここで落ちる。027 が鍵が合う標本の遅れで落ちる側の対照） | 境界、陽性 | `a_slow_unmatched_sample_is_not_judged_against_the_182_ms_line` |
| T-RP-85-041 | 026 の `e3q1` で、標本 4 の f を P7 の f にして（鍵が合う文）、訳の段は 418540 µs のまま、reuse は付けない | 遅い標本が製品の鍵では合う側 | 鍵が合う標本は同じ `id` の reuse を持ち、最大 ≤ 182 ms | `Err` で `e3q1` と標本 4（`utt 7 repeat 4`、`mic-74`）を名指し、`translate.draft.reused` が無いことと訳の段 418.540 ms を言う。別 `id` の対照でも同じ名指し（specified、3.21 の 2 の 1「鍵の写しを test に書かない。写しは製品の鍵から離れても test を通す」。製品の関数で合うのに遅い標本を、合わない側へ入れて 182 ms から逃がす build は、ここで落ちる。040 が遅い標本を合わない側に置く形の対） | 境界、陰性 | `a_slow_sample_whose_pair_the_product_key_matches_is_judged` |
| T-RP-85-042 | 標本の分けの関数に、9 組の（d, f）を 1 標本ずつ渡す。合う側は 001（P7）、002（P11）、003（P20）、004（`ところで、昨日は雨でした` と `ところで昨日は雨でした。`）、005（`被害は三件でした` と `被害は3件でした`）の 5 組。合わない側は 009（`これは本当ですか` と `これは本当ですか？`）、010（`すごいですね` と `すごいですね！`）、011（`連合国が` の d と P7 の f）、012（`三件と五件です` と `3件と5件と7件です。`）の 4 組 | 分けが製品の鍵に従う。合う 5 組と合わない 4 組 | 合う側は鍵が合う標本、合わない側は合わない標本 | 5 組が鍵が合う標本、4 組が合わない標本に分かれる（specified、RP-85 の (1) と 001 から 005、009 から 012 の期待値。分けは `hayamimi_relay::partial::reuse_match`（`japanese` は真）の答えだけで決め、規則を test に写さない。写した build は、製品の鍵を変えた時に 001 から 012 との食い違いで見つかる） | 陽性、陰性 | `the_sample_split_agrees_with_the_products_key_on_the_nine_pairs` |
| T-RP-85-043 | 2 run の列：名前が `e3p1` と `e3p2` で、どちらも F85q の既定（通る内容） | 窓 E3' の名前だけの列。内容は通る形 | E3'' の run が在る | `Err` で E3'' の run が無いことを名指す（specified、3.21 の 4「`e3p` の記録は記録として残り、test は読まない」。030 は標本が 0 の run、この行は run が 0 本。名前を替えただけの test は、旧名が通る build を落とせない。この行は旧名が通ると `Ok` になって落ちる） | 境界、陰性 | `a_set_of_runs_all_named_for_the_old_window_is_rejected_for_utterance_7` |
| T-RP-85-044 | 3 run の列：`e3p1`（F85q の既定で、標本 1 の reuse を除き、標本 3 を 182001 µs にした通らない内容）、`e3q1`（026 の形）、`e3q2`（026 の形） | 旧名の run は読まれない | E3'' の run だけ | `Ok`。`e3p1` を名指さない（specified、同上。T-RP-84-035 と同じ向き。旧名の run を別の理由で読む build は、`e3p1` の標本 1 と 3 で落ちる） | 境界、陽性 | `a_run_named_for_the_old_window_is_not_read_beside_valid_runs_for_utterance_7` |
| T-RP-85-045 | F85q の `e3q1` から `e3q1.e2e1.events.jsonl` を除く（`e3q1.e2e1.jsonl` と中継の stdout は在る）。`e3q2` は既定 | events の file の欠け | file が在る | `Err` で欠けた file の path を名指す。`e3q2` は名指さない（implicit、確定文と最後の速報を読めない run を、鍵が合う標本 0 と読んで別の理由で落とさず、0 件で通さない。021 と同じ向き） | 陰性 | `a_missing_events_file_is_rejected_by_path` |
| T-RP-85-046 | 026 の `e3q1` で、鍵が合う標本 3 の `d`（`(t_final_us − t_end_us)/1000 − final_latency_ms`）を 100 ms にし、訳の段を 182001 µs にする（`e3q2` は 026 のまま） | 有効の線（`d ≥ 120 ms`）に届かない、鍵が合う標本 | 有効の線を掛けない | `Err` で `e3q1` と標本 3（`utt 7 repeat 3`、`mic-73`）、訳の段 182001 µs を名指す。別 `id` の対照でも同じ名指し（specified、設計 RP-85 の行と 3.21 の 2 の 1「有効の線は掛けない。訳の段は `d` を含まないため」。有効の線を掛けて標本 3 を外す build は、最大が 182000 に戻って通し、ここで落ちる。026 と 027 の標本の `d` は 500 ms で、線を掛けても結果が変わらない） | 境界、陰性 | `a_keyed_sample_below_the_validity_line_is_still_judged` |
| T-RP-85-047 | 026 の `e3q1` で、標本 3 の reuse（`mic-73`）を除き、`t_us` が標本 3 の `t_final_us` と違う `final` を 1 つ足す（`id` は `mic-73`、確定文は f の `連合国は` を `連合国が` にした文、その前の速報は P7 の d。events の並びでは本物の `final` の後ろ。偽の `final` の `t_us` は標本 3 の `t_final_us` の 1 µs 後で、標本 4 の速報の `t_us` より前に置く。標本 4 の速報は偽の `final` の後ろに在るので、標本 4 の区切りの始まりは偽の `final` になり、標本 4 は今までどおり `key` の側に入る。偽の `final` の `t_us` を標本 4 の速報より後に置くと、標本 4 は `no_draft` になる） | `id` が同じで `t_us` が違う `final` | 標本と確定文は `t_final_us` と `t_us` で結ぶ | `Err` で `e3q1` と標本 3（`utt 7 repeat 3`、本物の `final` の `mic-73`）を名指し、`translate.draft.reused` が無いことを言う。別 `id` の対照でも同じ名指し（specified、3.21 の 2 の 1「標本と確定文は `t_final_us` と `final` の `t_us` で結ぶ」。時刻で結ぶ実装は本物の確定文を選び、標本 3 が鍵が合うのに reuse が無いとして落ちる。`id` で結ぶ実装は後ろの確定文を選んで標本 3 を合わない側に入れ、通してしまうので、ここで落ちる） | 境界、陰性 | `a_sample_is_joined_to_its_final_by_time_not_by_id` |
| T-RP-85-048 | 3 run の列：F85q の既定の内容で、名前が `e3q1x`、`E3Q1`、`xe3q1` | 名前の境界。`e3q` と数字だけでない物 | E3'' の run が在る | `Err` で E3'' の run が無いことを名指す（derived、設計の「読む run の頭の字を `e3q` に」と 4.76 の 036 と同じ読み。旧名の対照 043、044 は `e3p` だけを見るので、名前の境界はこの行が持つ） | 境界、陰性 | `names_other_than_e3q_and_digits_are_not_read_for_utterance_7` |
| T-RP-85-049 | 2 入力。026 の `e3q1` で、(a) 標本 3（`mic-73`。訳の段は 418540 µs にする）の `final`（`t_us` が標本 3 の `t_final_us` に等しい行）を events から除く。(b) (a) に加えて、`id` が `mic-73`、`t_us` が標本 3 の `t_final_us` より 1000000 µs 後、text が P7 の f の `連合国は` を `連合国が` にした文（`reuse_match` が `None` を返す文）の `final` を 1 つ足す。標本 3 の前の速報（P7 の d）は残す。`e3q2` は 026 のまま | 確定文に結べない標本（`final` が無い。(b) は `id` が同じで `t_us` が違う `final` が在る） | 標本と確定文は `t_final_us` と `t_us` で結ぶ | どちらも `Err` で `e3q1`、`utt 7 repeat 3` と `t_final_us` の値を名指す。結べない標本なので、`id` の代わりに `t_final_us` が出て、`mic-73` は名指しに現れない（(b) の足した `final` の `id` も現れない）。標本 3 を合わない側に並べた出力は無く、`e3q2` は名指さない。別 `id` の対照（標本の行に `mic-99`）でも同じ名指しで、`mic-99` は出ない（specified、3.21 の 2 の 6 と 7、RP-85 の (4)「`t_final_us` に等しい `t_us` の `final` が events に無い標本は、合わない側に入れず、run を標本の `utt 7 repeat <N>` と `t_final_us` を名指して落とす」。結べない標本を合わない側へ入れる build は、遅い標本 3 が 182 ms の判定から外れ、鍵が合う標本 1、2、5 の 3 つで通るので、(a) で落ちる。(b) は `id` で結ぶ build を落とす（その build は足した `final` を選び、`None` の標本 3 を `key` で合わない側に入れ、鍵が合う標本 1、2、5 の 3 つと最大 182 ms 以下で通すので、`Err` を期待するこの行で必ず落ちる）。047 の逆の向き） | 境界、陰性 | `a_sample_whose_final_is_missing_from_the_events_fails_the_run_by_name` |
| T-RP-85-050 | 026 の `e3q1` で、標本 3 の区切り（標本 2 の `final` の後から標本 3 の `final` の前まで）から `partial` を除く（標本 2 の区切りの `partial`、つまり P7 の d は残る）。標本 3 の `translate.draft.reused` も除く。標本 4 は 026 のまま合わない（`key`）。`e3q2` は 026 のまま | 確定文には結べるが、区切りの中に `partial` が無い標本。1 つ前の回の速報は在る | 合わない側へ `no_draft` で入る | `Ok`。出力の `e3q1` の合わない標本は標本 3（`utt 7 repeat 3`、`mic-73`、理由 `no_draft`）と標本 4（`utt 7 repeat 4`、`mic-74`、理由 `key`）の 2 件で、鍵が合う標本は 1、2、5 の 3（specified、3.21 の 2 の 6 と RP-85 の (4)・陽性欄「区切りの中に `partial` が無い fixture が通り、その標本が理由 `no_draft` で出る」。標本 3 に reuse が無くても落ちない。1 つ前の `final` より前の速報（標本 2 の速報）を鍵に掛ける build は、標本 3 を鍵が合う側に入れ、reuse の欠けで落ちる） | 境界、陽性 | `a_sample_with_no_draft_in_its_span_is_listed_as_no_draft_and_not_matched` |
| T-RP-85-051 | 050 で、標本 5 の f を P7 の f の `連合国は` を `連合国が` にした文にして（標本 5 の reuse も除く）、鍵が合う標本を 1 と 2 の 2 つだけにする（標本 3 は `no_draft`、標本 4、5 は `key`）。標本 3 の f は P7 の f のまま | `no_draft` の標本は鍵が合う標本の数に入らない（入れれば 3） | 鍵が合う標本 ≥ 3 | `Err` で `e3q1`、鍵が合う標本が 2 であること、「3 以上」を名指し、合わない標本 3 件（`utt 7 repeat 3`、`mic-73` の `no_draft`、`utt 7 repeat 4`、`5`（`mic-74`、`mic-75`）の `key`）を理由つきで並べる。`e3q2` は名指さない。別 `id` の対照でも同じ名指し（specified、3.21 の 2 の 6「3 以上の数には入れず」。標本 3 の速報を補って鍵が合うことにして数える build は 3 つになって通し、ここで落ちる。050 が 3 つ揃う側の陽性対照） | 境界、陰性 | `a_no_draft_sample_is_not_counted_among_the_keyed_samples` |
| T-RP-85-052 | 2 入力。026 の `e3q1` で、(a) 標本 3 の行から `repeat` の欄を除く。(b) 標本 3 の行の `repeat` を文字列の `"3"` にする。`e3q2` は 026 のまま | `repeat` が無い行と、`repeat` が JSON の整数でない行（記録として壊れた標本の行） | 標本の行の `repeat` は JSON の整数 | どちらも `Err` で、診断に `e3q1` と、標本 3 の行の `t_final_us` の値が在り、標本の名指しが空でない（`repeat` の値が空の名指しや、名指しの無い診断で通さない）。`e3q2` は名指さない（specified、3.21 の 2 の 7「`repeat` の欄が無い行と、`repeat` が JSON の整数でない行は、記録として壊れているので、その行の `t_final_us` を名指して run を落とす（名指しが空の形で通さない）」。`repeat` を読めない行を黙って落とす build や、名指しを空にして `Err` を返す build は、診断に `t_final_us` の値が無いので、ここで落ちる。026 が `repeat` が整数の陽性対照） | 境界、陰性 | `a_sample_row_without_an_integer_repeat_fails_the_run_by_its_final_time` |

**変異の確かめ**（検証の席が手で打つ。test の行ではない。設計の陰性欄）：

- 鍵の関数を今の形（同じか `。` を足しただけ）に戻した build で、001 から 005 が `translate.draft.reused` 0 件で落ちる。
- V の数の検査を外した build（または台本）で、016 が `verdict` `ok` になって落ちる。
- 数字を全部 1 つの印にまとめる build で、012 が `normalized` で合って落ちる。
- 広げた鍵を en にも掛ける build で、013 が落ちる。
- 鍵が合う標本の reuse を、件数だけで数える build は、038（reuse を消す）と 039（別の `id`）で通して落ちる。
- 合わない標本を 182 ms の判定に入れる build は 040 で、遅い標本を合わない側へ入れて 182 ms から逃がす build は 041 で落ちる。
- 鍵の規則を test に写した分けは、製品の鍵を変えた時に 042 が落ちる。
- 旧名（`e3p`）の run を読む build は、043 と 044 で落ちる。
- 確定文に結べない標本を合わない側へ入れる build は 049 で、1 つ前の回の速報を鍵に掛ける build は 050 で、`no_draft` の標本を鍵が合う標本に数える build は 051 で、`repeat` を読めない行を名指しの空な形で通す build は 052 で落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 発話 7 の標本は `t_final_us` と `t_tr_us` が在る標本で、有効の線（`d ≥ 120 ms`）は掛けない（046 が `d` 100 ms の鍵が合う標本で固定する。026 から 029 の標本の `d` は 500 ms で、線を掛けても通るので固定しない）。標本と確定文は `t_final_us` と `t_us` の時刻で結び、`id` では結ばない（047）。対は（`utt`、確定文、速報）の組で重ねない（018 から 020）。
- 016 の E の依頼の上限 `PROMPT` は T-RP-69-018 の定数に合わせた。値は RP-69 の実装が持つ。
- 合わない標本を出力に並べる書式。設計は出力の理由の欄を `key`（鍵が `None`）か `no_draft`（確定文の前の区切りに速報が無い）と決めている。test は、標本ごとの 1 行に、run の名前、`id`、訳の段の値の字、理由の語が在ることだけを見る（026、034、036、040、051）。
- 窓 E3'' の ja の 2 本の run の events の file の名前は、設計の RP-85 の行が書く `e3q<N>.e2e1.events.jsonl` である。
- 031 の `e3q<N>.e2e1.jsonl` の run が、RP-85 の (4) で 2 本（`e3q1`、`e3q2`）であること。設計は窓 E3'' の ja の 2 本と書く(W と X の対は止めた、H132)。

### 4.78 RP-86（早い終わりの detector C と持った候補の短い無音の確定 D。止めた）

**止めた。** C と D を両方外した。`rp86.rs` は作らない。H132。この節の行は実装されず、打たれない。R8x-5 の offline の log（`docs/results/r8v/extra.md`）は記録として残る。
test：`native/hayamimi-eval/tests/rp86.rs`。受入 test の名前は `rp86_extra_early_notices_shorten_the_ja_wait_without_mid_sentence_splits`（未実装、持ち主 R8x-6b）。実走の行（T-RP-86-047）だけが `#[ignore]` を持ち、`docs/results/r8v/` の offline の log と `docs/results/r8x/` の記録を読む。記録が置かれるまでは走らせない。ほかの行は fixture を読み、`--ignored` を付けずに通る。
command：fixture の行は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp86`。実走の行はこれに `-- --ignored` を足す。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない（047 だけは受入 test の名前の全部）。判定の関数の名前と引数の形は実装の席が決める。行は「dir（offline の log、run の file、stdout、events、machine）を渡す」形で書く。
設計は D 3.19 の「割れを増やさない守りと、その測り」と、D 6 と 7 の RP-86、D 3.17 の「RP-66 と RP-67 の数え方」である。
W は今の規則で C と D を切った形(`--early-extra off`)、X は今の規則に C と D を足した形(`--early-extra on`)。方針 B(`--early-complete nouns`)は外したので、W と X のどちらにも入らない(H130)。

**数え方**（D 3.17 の数え方の 1、7、8 と RP-66 の T のとおり。この席は足さない）：

- 標本の枠は ja が 1 run に 23 × 5 = 115 で、X の 2 本で 230、W の 2 本で 230。en が 1 run に 15 × 5 = 75。有効は `final` が在り `d ≥ 120`。`d` の p50、p95 は nearest-rank（順位は (p × n + 99) / 100 の整数除算）で、言語ごとに W の 2 本（en は 1 本）と X の 2 本（en は 1 本）をそれぞれ合わせた集まりで取る。run ごとの値は記録だけ。
- 「文の途中」の割れは、RP-66 の T-RP-66-040 から 047 の仕分け（本物の文の切れ目、尾の雑音、文の途中）で数える。参照 J5 の前の文が e = 11 で本物、e = 9 で文の途中、参照 J4 の後ろの文が `ん` で尾の雑音。
- 文の途中の切断の率は、文の途中の数 ÷ 枠の全標本（ja の X の 2 本で 230）。上限 10 % は整数で `文の途中の数 × 10 ≤ 枠の標本の数` と書く（230 の 10 % は 23 件。23 は通り、24 は落ちる）。
- 品質の式は RP-66 の行のとおり：「final なし」の数と、訳の取れない有効な標本の数が W より増えない。ja の CER（X の 2 本の `cer_dist` の和 ÷ `cer_denom` の和）が W 以下。比べるときは、割り算を使わず `X の dist × W の denom ≤ W の dist × X の denom` で見る。
- 時刻は µs の整数、`final_latency_ms` は 10 の -6 乗の整数に読んで比べる。

**fixture**（式で作る。実測のふりをしない）：

- **F86J**（ja の W の 2 本）：1 run は枠 115 = 有効 100 + final なし 15。2 本を合わせた有効 200 標本の k 番目（k = 1〜200、`d` の小さい順）の `d` は `400 + 4k` ms。奇数の k は 1 本目（`w1.e2e1`）、偶数の k は 2 本目（`w2.e2e1`）。したがって p50 は 100 番目の 800 ms、p95 は 190 番目の 1160 ms。有効の標本は全部 `L_dec` 100 ms、`L_tr` 300 ms、`finals` 1、`cer_dist` 1、`cer_denom` 50。final なしの 15 は `t_final_us` が null、`finals` 0、`cer_dist` 1、`cer_denom` 50（F66J と同じ）。2 本の `cer_dist` の和は 230、`cer_denom` の和は 11500。
- **F86X**（ja の X の 2 本）：F86J の k 番目から 92 ms を引いた物（`x1.e2e1`、`x2.e2e1`）。p50 は 708 ms、p95 は 1068 ms。ほかの欄は F86J と同じ。
- **F86E**（en）：W の 1 本（`w1.e2e2`）は F66E の式。75 有効で、k 番目の `d` は k ≤ 38 で 262 + k、39 ≤ k ≤ 72 で 520 − (72 − k)、73 ≤ k で 600 + 10 × (k − 73)。p50 は 38 番目の 300 ms、p95 は 72 番目の 520 ms。X の 1 本（`x1.e2e2`）は既定で W と同じ標本。
- **F86L**（offline の log）：`extra-lat.log` と `extra-heldout-ja.log`。どちらも JSON の行で、1 行が 1 つの組（`set`）と 1 つの腕（`arm`）を持つ。`set` は `lat-ja`、`lat-en`、`heldout-ja`、`arm` は `W` か `X`、2 つの数は JSON の整数。`extra-lat.log` は lat-ja の 2 行、`extra-heldout-ja.log` は heldout-ja の 2 行。W の値は、lat-ja が `continuing_confirm`（後ろに声が続く確定）3、`b_only_split`（B だけの割れ）4、heldout-ja が 9 と 9。これは live の値(RP-86 の (0) の陽性対照「W の腕が live の値を再現する」。再現しない時は X と比べずに落ちる)。X の値は既定で W と同じ。例：`{"set":"lat-ja","arm":"W","continuing_confirm":3,"b_only_split":4}`。X の値は式で置いた値で、probe の実測ではない。欄の欠けた行と読めない行は 0 件と読まず落ちる（RP-86 の (0)）。
- **F86S**（X の ja の各 run の stdout と events）：`asr.endpoint.decided` が 20 行（`id` は 1〜20 の数）。`id` が 3、7、9 の行は `notice` が `c`、`id` が 12 の行は `rule` が `held_silence` で `notice` が `none`、残りは `notice` が `b` か `none`。events に `final` が 20 件（`id` は `mic-1`〜`mic-20`。text は全部 `今日は晴れです。`）。W の各 run の stdout は `c` と `held_silence` を持たない。
- **F86M**：6 run（ja の `w1.e2e1`、`x1.e2e1`、`w2.e2e1`、`x2.e2e1`、en の `w1.e2e2`、`x1.e2e2`）がそれぞれ F83 の形の `<run>.machine.jsonl` を持つ。
- **S86**（割れ）：「文の途中の割れ 1 件」は、有効な標本 1 つの `finals` を 2 にして、2 つの `final` の text を参照 J5 の先頭 9 字と残りにした物。本物は先頭 11 字と残り、尾の雑音は参照 J4 の先頭 11 字と `ん`。割れは別々の有効な標本に置く。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-86-001 | F86J、F86X、F86E、F86L、F86S、F86M の dir そのまま（割れは 0 件） | 設計の陽性の case。(0) から (6) が全部内側 | 全部成立 | `Ok`（specified、RP-86 の陽性欄「offline の log が W と同じ数を持ち、X の ja の `d` を W の各標本から 92 ms 引き、割れを足さない組が (0)〜(6) を通る」。92 ms の引きは p95 の差 1160 − 1068 = 92 ms） | 陽性 | `a_run_set_shaped_like_the_design_passes_every_check` |
| T-RP-86-002 | 001 で、F86L の X の 4 つの値を W より小さくする（lat-ja の X の行が `continuing_confirm` 2 と `b_only_split` 3、heldout-ja の X の行が 8 と 8。W は 3 と 4、9 と 9 のまま） | X が W より小さい | 各値で X ≤ W | `Ok`（specified、RP-86 の (0)「W 以下」。等号は 001 が持つ） | 境界、陽性 | `x_counts_below_w_in_the_offline_log_pass` |
| T-RP-86-003 | 001 で F86L の lat-ja の X の行の `continuing_confirm`（後ろに声が続く確定）を 4（W の 3 より 1 件多い）にする | 設計の陰性の case。lat-ja の確定 | X ≤ W | `Err` で lat-ja と「後ろに声が続く確定」の 4 と 3 を名指す。ほかの値は名指さない（specified、RP-86 の陰性欄、(0)） | 境界、陰性 | `one_more_continuing_confirm_in_lat_ja_is_rejected` |
| T-RP-86-004 | 001 で F86L の heldout-ja の X の行の `continuing_confirm` を 10（W の 9 より 1 件多い）にする | held-out の確定 | 同上 | `Err` で held-out の ja と 10 と 9 を名指す（specified、同上。lat-ja が内側でも、held-out が外れれば落ちる） | 境界、陰性 | `one_more_continuing_confirm_in_the_heldout_is_rejected` |
| T-RP-86-005 | 001 で F86L の lat-ja の X の行の `b_only_split`（B だけの割れ）を 5（W の 4 より 1 件多い）にする | lat-ja の割れ | 同上 | `Err` で lat-ja と「B だけの割れ」の 5 と 4 を名指す（specified、RP-86 の (0)） | 境界、陰性 | `one_more_b_only_split_in_lat_ja_is_rejected` |
| T-RP-86-006 | 001 で F86L の heldout-ja の X の行の `b_only_split` を 10（W の 9 より 1 件多い）にする | held-out の割れ | 同上 | `Err` で held-out の ja と 10 と 9 を名指す（specified、同上） | 境界、陰性 | `one_more_b_only_split_in_the_heldout_is_rejected` |
| T-RP-86-007 | 陽性対照の後ろで W と X を比べる関数 `within()` を直に呼ぶ 2 入力(dir は渡さない)。(a) W 0、X 0。(b) W 0、X 1 | 値が 0 の境界。W が live の値を再現するかの確かめは通らない入力で、比べる関数だけを打つ | X ≤ W | (a) は `Ok`、(b) は `Err` で 1 と 0 を名指す（specified、RP-86 の (0)「W 以下」。0 件の W に 1 件を足すと落ちる。W を 0 にした F86L の dir は live の値(3 と 4、9 と 9)を再現せず、陽性対照で落ちるので、dir では打たない) | 境界 | `a_zero_baseline_allows_zero_and_rejects_one` |
| T-RP-86-008 | 001 で `extra-lat.log` を除き、`x1.e2e1.jsonl` の中身を JSON でない文字列にする | log が無い時の順 | log が先 | `Err` で `extra-lat.log` の path を名指す。`x1.e2e1.jsonl` の読めなさは名指さない（specified、RP-86 の (0)「log が無い時は E2E の file を読まずに落ちる」。E2E の file を先に読むと読めなさで落ちるので、順が分かる） | 陰性 | `a_missing_offline_log_fails_before_any_e2e_file_is_read` |
| T-RP-86-009 | 001 で `extra-heldout-ja.log` の heldout-ja の X の行から `b_only_split` の欄を除く（W の行は 9 のまま） | 値の欠け | 全値が在る | `Err` で heldout-ja の X の行と `b_only_split` が無いことを名指す（specified、RP-86 の (0)「欄の欠けた行は落ちる」。欠けた値を 0 と読んで W 以下に通さない） | 陰性 | `a_missing_counter_in_the_offline_log_is_rejected_not_read_as_zero` |
| T-RP-86-010 | F86J と F86X（001 の ja） | 設計の差 92 ms | p95 の差 ≥ 60 ms、p50 ≤ W | `Ok`。出力に W の p50 800、p95 1160、X の p50 708、p95 1068、差 92 が並ぶ（specified、RP-86 の (1)。F86 の式から derived） | 陽性 | `x_92_ms_faster_at_every_sample_passes_the_ja_wait` |
| T-RP-86-011 | 010 で X の各標本を F86J の対応する標本から 60 ms 引いた物（p95 は 1100 ms） | p95 の差の線ちょうど | 差 ≥ 60 ms | `Ok`（specified、RP-86 の (1)「60 ms 以上小さく」。等号側。p50 は 740 ms で 800 ms 以下） | 境界、陽性 | `a_p95_gain_of_exactly_60_ms_passes` |
| T-RP-86-012 | 010 で X の各標本を 59.999 ms 引いた物（p95 は 1100.001 ms） | p95 の差の線の 1 桁下（10 の -3 乗ms） | 同上 | `Err` で ja の p95 の差 59.999 と 60 を名指す。p50 は名指さない（specified、同上。p50 は 740.001 ms で W 以下なので理由は p95 だけ） | 境界、陰性 | `a_p95_gain_of_59_999_ms_is_rejected` |
| T-RP-86-013 | X の標本を、k ≤ 100 で F86J と同じ、k > 100 で `800 + 3 × (k − 100)` ms にした物（p50 は 800 ms、p95 は 1070 ms） | p50 が W と等しい。p95 の差は 90 ms | p50 ≤ W | `Ok`（specified、RP-86 の (1)「p50 が W 以下」。等号側） | 境界、陽性 | `an_x_p50_equal_to_w_passes` |
| T-RP-86-014 | 013 で X の 100 番目を 800.001 ms にする（101 番目は 803 ms） | p50 の線の 1 桁上 | 同上 | `Err` で ja の p50 の 800.001 と 800 を名指す。p95 は名指さない（specified、同上。p95 の差は 90 ms で内側） | 境界、陰性 | `an_x_p50_one_step_over_w_is_rejected` |
| T-RP-86-015 | X の 1 本目は有効 100 標本の `d` を全部 600 ms、2 本目は全部 1000 ms にする。W は F86J | 2 本を合わせる（run ごとでない） | 合わせた集まり | `Ok`。合わせた X の p50 は 100 番目の 600 ms、p95 は 190 番目の 1000 ms で、差 160 ms（specified、D 3.17 の数え方の 7、T-RP-66-039 と同じ向き。2 本目だけで取ると p50 が 1000 ms で W の 800 ms を超えるので、run ごとに判定する build はここで落ちる）。run ごとの値は記録だけに出す | 陽性、陰性 | `the_two_x_runs_are_judged_together_not_one_by_one` |
| T-RP-86-016 | X の 2 本とも全標本の `t_final_us` を null にする | 有効が 0 件 | 有効が在る | `Err` で ja の X に有効な標本が無いことを名指す（implicit、空の p50 と p95 を作らず、0 件を通さない。T-RP-66-048 と同じ向き） | 境界、陰性 | `an_x_set_with_no_valid_sample_is_rejected` |
| T-RP-86-017 | 010 の X の最小の標本（312 ms）を (a) 120 ms にした入力と、(b) 119 ms にした入力 | 有効の線 0.12 秒の両側 | `d ≥ 120` が有効 | (a) は有効 200、p50 708、p95 1068 で `Ok`。(b) は 119 ms を終端不正として数えず、有効 199、p50 712（100 番目 = F86X の 101 番目）、p95 1072（190 番目 = F86X の 191 番目）で `Ok`。出力の n と p50 と p95 で、線の位置が分かる（derived、F86X の式。D 3.17 の数え方の 3） | 境界 | `the_valid_line_is_120_ms_for_the_ja_wait` |
| T-RP-86-018 | S86。W の 2 本に文の途中の割れを合わせて 23 件、X の 2 本にも 23 件（X は 1 本目に 12 件、2 本目に 11 件） | 設計の割れの数が等しい。枠 230 の 10 % ちょうど | X ≤ W かつ 率 ≤ 10 % | `Ok`（specified、RP-86 の (2)。X の率は 23 ÷ 230 = 10.0 % で、`23 × 10 ≤ 230`。分母が有効な標本の 200 なら 11.5 % になって落ちるので、分母が枠の全標本であることもここで固定する） | 境界、陽性 | `mid_sentence_cuts_at_the_ten_percent_cap_of_the_frame_pass` |
| T-RP-86-019 | S86。2 入力。(a) W に文の途中の割れを 25 件、X に 24 件。(b) W に 30 件、X に 25 件 | W 以下だが 10 % を超える（評議の指摘。陰性対照） | 率 ≤ 10 % | どちらも `Err` で、X の率が 10 % を超えること（24 ÷ 230 = 10.43 %、25 ÷ 230 = 10.87 %）を名指す。「X が W より多い」とは名指さない（specified、RP-86 の (2)「X の文の途中の切断の率も ja 10 % 以下」。X ≤ W は (a) の 24 ≤ 25 と (b) の 25 ≤ 30 で成り立っているので、W との比べだけの判定は通してしまう。理由を絶対の上限だけに絞った入力） | 境界、陰性 | `x_below_w_but_over_ten_percent_of_the_frame_is_rejected` |
| T-RP-86-020 | S86。2 入力。(a) W に 20 件、X に 21 件。(b) W に 0 件、X に 1 件 | X が W より 1 件多い。率は 10 % の内側（(a) は 9.13 %、(b) は 0.43 %） | X ≤ W | どちらも `Err` で、X の件数が W を超えること（21 と 20、1 と 0）を名指す。率の上限は名指さない（specified、RP-86 の (2)「X の 2 本の和で W の 2 本の和以下」。019 の対。比べの理由だけに絞った入力。割れを足した fixture が落ちる、設計の陰性の case） | 境界、陰性 | `one_more_mid_sentence_cut_than_w_is_rejected` |
| T-RP-86-021 | S86。W に 0 件、X に 0 件 | 割れが無い | X ≤ W、率 ≤ 10 % | `Ok`（specified、同上。率は 0 ÷ 230 で、0 で割らない） | 境界、陽性 | `no_cut_in_either_arm_passes` |
| T-RP-86-022 | S86。X の 2 本に割れを 30 件。内訳は文の途中 23、本物の文の切れ目 5、尾の雑音 2。W に文の途中を 23 件 | 割れの数は 10 % を超えるが、文の途中だけが分子 | 文の途中だけを数える | `Ok`（specified、D 3.17 の数え方の 8。本物と尾の雑音は文の途中に入らない。X の割れ 30 ÷ 230 = 13.0 % で、全部の割れを分子にする build は落ちる） | 境界、陽性 | `only_mid_sentence_cuts_count_toward_the_rate` |
| T-RP-86-023 | en の X（`x1.e2e2`）に文の途中の割れを 10 件、W（`w1.e2e2`）に 0 件。ja は 001 のまま | en は文の途中の割れを判定に入れない | ja だけ | `Ok`（specified、RP-86 の (2)「ja 10 % 以下」と (4)。en の判定は `d` だけ。en の割れは記録に出す） | 境界、陽性 | `en_cuts_are_recorded_and_not_judged` |
| T-RP-86-024 | 001 で X の 1 標本（`d` が最大の 1108 ms の標本）を `t_final_us` が null の標本にする。有効 199、「final なし」は 31（W は 30） | 「final なし」が 1 件多い | X ≤ W | `Err` で「final なし」の 31 と 30 を名指す。p50 と p95 は名指さない（specified、RP-86 の (3)、RP-66 の品質の式。`d` は 100 番目 708、190 番目 1068 で内側） | 境界、陰性 | `one_more_sample_without_a_final_than_w_is_rejected` |
| T-RP-86-025 | 2 入力。(a) W の有効な標本のうち 2 標本、X の有効な標本のうち 2 標本の `t_tr_us` を null にする。(b) X の 3 標本にする | 訳の取れない有効な標本の数が等しい、1 件多い | X ≤ W | (a) は `Ok`、(b) は `Err` で 3 と 2 を名指す（specified、RP-86 の (3)、RP-66 の品質の式。`d` は内側） | 境界 | `valid_samples_without_a_translation_may_equal_but_not_exceed_w` |
| T-RP-86-026 | 3 入力。X の 2 本の `cer_dist` の和が (a) 229、(b) 230、(c) 231。`cer_denom` の和は 11500。W は 230 と 11500 | CER が W より小さい、等しい、大きい | X ≤ W | (a) と (b) は `Ok`、(c) は `Err` で ja の CER を名指す（specified、RP-86 の (3)。等号は通す。割り算の丸めで外さず、`231 × 11500 ≤ 230 × 11500` の整数の比べで落とす） | 境界 | `ja_cer_may_equal_but_not_exceed_w` |
| T-RP-86-027 | F86E。X の各標本を W の対応する標本と同じにする（001 の en） | en の差が 0 | W の +60 ms 以内 | `Ok`（specified、RP-86 の (4)） | 陽性 | `en_equal_to_w_passes` |
| T-RP-86-028 | 027 で X の各標本に 60 ms を足した物（p50 は 360 ms、p95 は 580 ms） | W の +60 ms の線ちょうど | `X ≤ W + 60 ms` | `Ok`（specified、RP-86 の (4)「+60 ms 以内」。等号側。D 3.17 の数え方の 4 と同じ、ちょうど 60 は内） | 境界、陽性 | `en_exactly_60_ms_slower_than_w_passes` |
| T-RP-86-029 | 028 で X の 72 番目だけを 580.001 ms にする（71 番目は 579 ms、73 番目は 660 ms） | p95 の線の 1 桁上 | 同上 | `Err` で en の p95 の 580.001 と W の 520 + 60 を名指す。p50 は名指さない（specified、同上。p50 は 360 ms で内側） | 境界、陰性 | `en_p95_one_step_over_w_plus_60_ms_is_rejected` |
| T-RP-86-030 | 028 で X の 38 番目だけを 360.001 ms にする（37 番目は 359 ms、39 番目は 547 ms） | p50 の線の 1 桁上 | 同上 | `Err` で en の p50 の 360.001 と W の 300 + 60 を名指す。p95 は名指さない（specified、同上。p95 は 580 ms で内側） | 境界、陰性 | `en_p50_one_step_over_w_plus_60_ms_is_rejected` |
| T-RP-86-031 | F86S の X の ja の 2 本そのまま。stdout の `id` は数、events の `id` は `mic-` つきの文字列 | 設計の (5) の陽性。`c` が各 run に 3 件、`held_silence` が 1 件。`id` の型が違っても結べる | `c` が各 run に 1 件以上、すべて結べる | `Ok`。出力に `c` の件数（各 run 3）、`held_silence` の件数（各 run 1）、結べた `id` の数（各 run 4）が並ぶ（specified、RP-86 の (5)。`ltrimstr("mic-")` と文字列化で結ぶ、D 3.19 の置き換えの式と同じ） | 陽性 | `x_runs_with_c_notices_and_held_silence_tied_to_finals_pass` |
| T-RP-86-032 | 031 の `x2.e2e1` の `notice` が `c` の行を全部 `b` にする（`held_silence` は残す） | 2 本目だけ `c` が 0 件 | 各 run に 1 件以上 | `Err` で `x2.e2e1` と `c` が 0 件であることを名指す。`x1.e2e1` は名指さない（specified、RP-86 の (5)「X の ja の run ごとに `notice` が `c` の `asr.endpoint.decided` が 1 件以上」。「X の ja の run ごとに」。2 本を合わせた件数で読む build は、`x1.e2e1` の 3 件で通ってしまうので、ここで落ちる） | 境界、陰性 | `an_x_run_without_a_c_notice_is_rejected` |
| T-RP-86-033 | 031 の X の 2 本の stdout の `asr.endpoint.decided` から `notice` の欄を除く（今の build の形） | 欄が無い | `c` が在る | `Err` で `c` が 0 件であることを名指す（specified、同上。今の build の記録を X の位置に置いた形。欄が無い行を `c` と数えない） | 陰性 | `a_stdout_without_the_notice_field_has_no_c_notice` |
| T-RP-86-034 | 031 の `x1.e2e1` の stdout に、`id` が 21、`notice` が `c` の行を足す（events に `mic-21` の `final` は無い） | `c` の行が `final` と結べない | すべて結べる | `Err` で `x1.e2e1` と `id` の 21 を名指す（specified、RP-86 の (5)「`notice` が `c` の `id` がすべて `final` と結べ」。031 の結べる 3 件と対） | 境界、陰性 | `a_c_notice_that_ties_to_no_final_is_rejected` |
| T-RP-86-035 | 031 の `x1.e2e1` の stdout に、`rule` が `held_silence` で `id` が 22 の行を足す（events に `mic-22` の `final` は無い） | `held_silence` の行が `final` と結べない | すべて結べる | `Err` で `x1.e2e1` と `id` の 22 を名指す（specified、RP-86 の (5)「`held_silence` と `notice` が `c` の `id` がすべて `final` と結べ」） | 境界、陰性 | `a_held_silence_that_ties_to_no_final_is_rejected` |
| T-RP-86-036 | 031 の W の `w2.e2e1` の stdout に `notice` が `c` の行を 1 つ足す | W に `c` | W は 0 件 | `Err` で `w2.e2e1` と `c` の 1 件を名指す（specified、RP-86 の (5)「W の run には両方とも 0 件」） | 陰性 | `a_c_notice_in_a_w_run_is_rejected` |
| T-RP-86-037 | 031 の W の `w1.e2e1` の stdout に `rule` が `held_silence` の行を 1 つ足す | W に `held_silence` | W は 0 件 | `Err` で `w1.e2e1` と `held_silence` の 1 件を名指す。`c` は名指さない（specified、同上。理由を `held_silence` だけに絞った入力） | 陰性 | `a_held_silence_in_a_w_run_is_rejected` |
| T-RP-86-038 | 001 の W の ja の 2 本の stdout に `notice` が `b` の行と `none` の行を足し、X の stdout は 031 のまま | `c` でも `held_silence` でもない行は数えない | W は 0 件 | `Ok`（specified、同上。数える語が `c` と `held_silence` の 2 つだけであることを、W に `b` と `none` が在っても通ることで固定する。031 が数える側の陽性対照） | 境界、陽性 | `b_and_none_notices_in_a_w_run_are_not_counted` |
| T-RP-86-039 | 001 の ja の X の位置（`x1.e2e1`、`x2.e2e1`）に W の run（`w1.e2e1`、`w2.e2e1` の標本と stdout）を置いた dir | 設計の陰性の case。X が W と同じ | (1) と (5) が落ちる | `Err` で ja の p95 の差が 0 ms で線の 60 ms に届かないことと、`c` が 0 件であることの両方を名指す（specified、RP-86 の陰性欄「X の位置に W の run を置いた dir が (1) と (5) で落ちる」。offline の log と machine は 001 のまま） | 陰性 | `w_runs_in_the_x_position_fail_the_wait_and_the_notice_checks` |
| T-RP-86-040 | F86M の 6 run そのまま | 設計の (6) の陽性。全 run が RP-83 の判定を通る | 全 run が通る | `Ok`（specified、RP-86 の (6)。001 の machine） | 陽性 | `all_six_runs_pass_the_machine_check` |
| T-RP-86-041 | 040 で `x2.e2e1` の行 1 の `load15` を `2.50` にする | X の 1 run が RP-83 の線の値 | RP-83 の判定 | `Err` で `x2.e2e1`、`load15`、`2.50` を名指す。ほかの 5 run は名指さない（specified、RP-86 の (6)。RP-83 の T-RP-83-005 と同じ判定を呼ぶ。呼び忘れた build はここで落ちる） | 境界、陰性 | `an_x_run_beside_a_busy_machine_is_rejected` |
| T-RP-86-042 | 040 で `w1.e2e2`（en の W）の `<run>.machine.jsonl` を除く | machine の file の欠け | file が在る | `Err` で `w1.e2e2` と file が無いことを名指す（specified、RP-86 の (6) と RP-83 の「file の無い run も落ちる」） | 陰性 | `a_run_without_a_machine_file_is_rejected` |
| T-RP-86-043 | 040 で `w2.e2e1` の行 3 の `cargo` を `1` にする | run の途中の build | RP-83 の判定 | `Err` で `w2.e2e1`、`cargo`、`1` を名指す（specified、同上。T-RP-83-009 と同じ判定） | 陰性 | `a_cargo_in_the_middle_of_a_w_run_is_rejected` |
| T-RP-86-044 | 001 の dir から `x2.e2e1` の 3 つの file（標本、stdout、machine）を除く | X の ja が 1 本 | X の ja が 2 本 | `Err` で `x2.e2e1` が無いことを名指す（derived、RP-86 の (2)「X の 2 本の和」と D 3.17 の数え方の 7。1 本の p50 と p95 で通さない） | 境界、陰性 | `a_dir_with_one_x_ja_run_is_rejected` |
| T-RP-86-045 | 001 の dir から `w2.e2e1` の 3 つの file を除く | W の ja が 1 本 | W の ja が 2 本 | `Err` で `w2.e2e1` が無いことを名指す（derived、同上。W の 2 本の和） | 境界、陰性 | `a_dir_with_one_w_ja_run_is_rejected` |
| T-RP-86-046 | 001 の dir から `x1.e2e2` の 3 つの file を除く | X の en が 0 本 | X の en が 1 本 | `Err` で `x1.e2e2` が無いことを名指す（derived、RP-86 の (4)。en は W、X の 1 本ずつ） | 境界、陰性 | `a_dir_without_the_x_en_run_is_rejected` |
| T-RP-86-047 | 実走：`docs/results/r8v/` の `extra-lat.log` と `extra-heldout-ja.log`、`docs/results/r8x/` の `w<N>.e2e1.jsonl`、`x<N>.e2e1.jsonl`、`w1.e2e2.jsonl`、`x1.e2e2.jsonl`、各 run の `hayamimi` の stdout、`<run>.events.jsonl`、`<run>.machine.jsonl` | 実走 | 実走 | 001 の形で `Ok`（specified、RP-86）。R8x-5 の log と窓 E3 の記録が置かれるまでは走らせない | 陽性 | `rp86_extra_early_notices_shorten_the_ja_wait_without_mid_sentence_splits`（未実装、持ち主 R8x-6b） |
| T-RP-86-048 | 001 で `extra-lat.log` の lat-ja の X の行を途中で切る（`{"set":"lat-ja","arm":"X","continuing_confirm":2,"b_` で終わる） | 読めない行 | 全行が読める | `Err` で `extra-lat.log` と読めない行を名指す（specified、RP-86 の (0)「欄の欠けた行と読めない行は落ちる」。読めた行だけで W 以下に通さない） | 境界、陰性 | `an_unreadable_row_in_the_offline_log_is_rejected` |
| T-RP-86-049 | 001 で `extra-lat.log` の lat-ja の X の行の `continuing_confirm` を JSON の文字列 `"2"` にする | 数が JSON の整数でない | 2 つの数は JSON の整数 | `Err` で lat-ja の X の行と `continuing_confirm` を名指す（specified、RP-86 の (0)「2 つの数は JSON の整数」。文字列を数に読んで W 以下に通さない） | 境界、陰性 | `a_count_written_as_a_json_string_is_rejected` |
| T-RP-86-050 | 031 の `x1.e2e1` で、events の `mic-12`（stdout で `rule` が `held_silence` の行に結ぶ `final`）の text を 3 入力にする。(a) `明日は雨ですが。`（接続の形 `が`）、(b) `雨なので。`（接続の形 `ので`）、(c) `雨ですけど？`（接続の形 `けど`。末尾が `？`） | held_silence の `final` が接続の形で終わる | 0 件 | 3 入力とも `Err` で `x1.e2e1`、`held_silence`、`mic-12` と、終わりの語（`が`、`ので`、`けど`）を名指す（specified、RP-86 の (5)「`。？！` を除いた text が `JA_CONNECTIVE` か `JA_PARTICLE` の語で終わる `held_silence` の `final` が 0 件」。(c) は `？` を除いてから見る。語は `native/hayamimi-asr/src/endpoint.rs` の `JA_CONNECTIVE`。`ので` は `で` でも終わるが、長い語を名指す） | 境界、陰性 | `a_held_silence_final_ending_in_a_connective_is_rejected` |
| T-RP-86-051 | 031 の `x1.e2e1` で `mic-12` の text を 2 入力にする。(a) `これを。`（助詞 `を`）、(b) `ここまで。`（助詞 `まで`） | held_silence の `final` が助詞で終わる | 0 件 | 2 入力とも `Err` で `x1.e2e1`、`held_silence`、`mic-12` と、終わりの語（`を`、`まで`）を名指す（specified、同上。語は `JA_PARTICLE`。050 の接続の形と別の表の側） | 境界、陰性 | `a_held_silence_final_ending_in_a_particle_is_rejected` |
| T-RP-86-052 | 031 の `x1.e2e1` で `mic-12` の text を `ところが今日は晴れです。` にする | 接続の形が文の途中に在り、終わりでない | 終わりの語だけを見る | `Ok`（specified、同上。部分一致で見る build は `が` で落とす。050 と 051 が落ちる側の陽性対照） | 境界、陽性 | `a_connective_inside_the_sentence_is_not_a_tail` |
| T-RP-86-053 | 031 の `x1.e2e1` で `mic-12` の text は 031 のまま、`id` が 3 で `notice` が `c` の行の `final`（`mic-3`）の text を `明日は雨ですが。` にする | 接続の形で終わるが、`held_silence` の行でない `final` | `held_silence` の `final` だけを見る | `Ok`（specified、同上「`held_silence` の `final` が 0 件」。`c` の知らせで確定した `final` は数えない。050 が数える側） | 境界、陽性 | `only_held_silence_finals_are_checked_for_a_connective_or_particle_tail` |
| T-RP-86-054 | 001 で F86L の lat-ja の W の行と X の行の `continuing_confirm` を、どちらも 2 にする(X ≤ W は成り立つが、W が live の値 3 と合わない) | W が live の値を再現しない | W が live の値(lat-ja で 3 と 4、heldout-ja で 9 と 9)を再現する | `Err` で lat-ja の W の `continuing_confirm` が 2 で live の 3 と合わないことを名指す。X との比べには進まない(specified、RP-86 の (0)「陽性対照：W の腕が live の値を再現する。再現しない時は X と比べずに落ちる」。001 が通ることが陽性対照) | 境界、陰性 | `a_w_arm_that_does_not_reproduce_the_live_values_fails_before_the_comparison` |

**変異の確かめ**（検証の席が手で打つ。test の行ではない。設計の陰性欄と評議の指摘）：

- 文の途中の切断の率の絶対の上限を外し、W との比べだけにした build で、019 が落ちる。
- 分母を有効な標本の数（200）にした build で、018 が落ちる。
- 分子を割れの全部にした build で、022 が落ちる。
- 2 本を合わせず run ごとに判定する build で、015 が落ちる。
- `notice` の欄が無い行を `c` と数える build で、033 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- C と D の機構（threshold、終わりの線、`min_silence`、持った文の扱い、`--early-extra off` と `--endpoint silence` の開き方）は、4.79 の RP-87 が持つ。この節の 050 から 053 は、記録の `final` の text から終わりの語を測る側だけを持つ。
- F86L の `lat-en` の行は記録だけで、判定に入れない(H127 の 8)。fixture は lat-ja と heldout-ja の行だけで書いた。
- 交互の順（W、X、W、X）は記録に残らない。test は順を見ない。

### 4.79 RP-87（C と D の機構：値、分かれ道、開く条件。止めた）

**止めた。** C と D を両方外した。`rp87.rs` は作らない。H132。この節の行は実装されず、打たれない。
test：`native/hayamimi-asr/tests/rp87.rs`。受入 test の名前は `rp87_c_and_d_fire_on_their_own_lines_and_d_never_ends_a_connective_or_particle_tail`（未実装、持ち主 R8x-6）。実走の行は無く、全部の行が合成した確率の列と持った候補の文を流し、model も音声も読まず、`--ignored` を付けずに通る。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-asr --test rp87`。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない（035 だけは受入 test の名前の全部）。公開の口の名前と引数の形は実装の席が決める。行は「確率の列（32 ms の window ごとの値）と持った候補を流し、知らせ、確定、`notice` の欄を読む」形で書く。
設計は D 3.19 の「C と D の機構の受入」と、C と D の機構、D 6 と 7 の RP-87 である。

**読み方**：

- 1 window は 512 sample（32 ms）。`S(v, n)` は確率 v の window を n 個並べた列。声の部分は `S(0.95, 10)`（検出器だけを見る行）か `S(0.95, 40)`（確定まで見る行。1.28 秒で、早い確定の最短の声 1 秒を超える）。無音の部分は `S(0.20, n)`。
- 終わりの知らせは、終わりの線より低い window が `min_silence`（0.12 秒 = 1920 sample）を超えるまで続いた時に出る。512 sample の window では ceil(1920 / 512) = 4 window である。したがって低い window が 3 個では出ず、4 個で出る（derived、3.17 の「区切りの時刻の式」の数え方）。
- 声の始まりの線は「以上」、終わりの線は「未満」で読む。C は始まり 0.80、終わり 0.65。D は始まり 0.50、終わり 0.35。B は始まり 0.65、終わり 0.50（D 3.17 の方針 1）。
- 「C の知らせ」「D の知らせ」「B の知らせ」は、その検出器が声の 1 区間の終わりを出すこと。持った候補は `State::hold` で置く文（`今日は晴れです。` を既定とし、持った範囲の終わりは window 40 の始まり = sample 20480）。
- 接続の形(`JA_CONNECTIVE`)、助詞(`JA_PARTICLE`)、文末の語(`JA_FINAL`)は、`native/hayamimi-asr/src/endpoint.rs` の表で、公開にする(H127 の 3。R8x-6 の席)。test は表を直に読み、語を書き写さない。表の語数は今 14、10、17 で、test は語数が 0 でないことも確かめる(空の表で「全部の語」の行が通るのを防ぐ)。
- 確率を直接受ける口の形は H126(R8x-6 の席が決める)。
- 知らせの window の数え方の細部(最初の低い window を数えるか)は記録だけで、行は固定しない(H127 の 4)。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-87-001 | C に `S(0.95, 10)` と `S(0.20, 4)` | C の終わりの知らせが出る | 低い window が 4 個 | C の知らせが 1 件。声の区間の始まりは sample 0（specified、RP-87 の (1)「0.65 未満が 0.12 秒続いた所で知らせる」。4 window = 2048 sample は 1920 以上） | 陽性 | `c_notifies_after_four_low_windows` |
| T-RP-87-002 | C に `S(0.95, 10)` と `S(0.20, 3)` | 低い window が 1 個足りない | 同上 | C の知らせが 0 件（specified、同上。3 window = 1536 sample は 1920 未満。001 の 1 つ下の境界） | 境界、陰性 | `c_does_not_notify_after_three_low_windows` |
| T-RP-87-003 | C に `S(0.95, 10)`、`S(0.20, 3)`、`S(0.95, 1)`、`S(0.20, 3)` | 低い window の続きが途中で切れる | 続いた長さで数える | C の知らせが 0 件（derived、「0.12 秒続いた所」。2 回の 3 個を足して 6 個と数える build は 1 件出して落ちる） | 境界、陰性 | `c_counts_a_run_of_low_windows_not_their_total` |
| T-RP-87-004 | C に `S(0.80, 10)` と `S(0.20, 4)` | 始まりの線の値 | 確率 ≥ 0.80 | C の知らせが 1 件（specified、RP-87 の (1)「0.80 以上で声を始め」。等号側。末尾が表に無い語の陽性対照にあたる、C の 0.80 ちょうど） | 境界、陽性 | `c_starts_speech_at_exactly_0_80` |
| T-RP-87-005 | C に `S(0.79, 10)` と `S(0.20, 4)` | 始まりの線の 1 つ下 | 同上 | C の知らせが 0 件（specified、同上「0.79 で始めず」） | 境界、陰性 | `c_does_not_start_speech_at_0_79` |
| T-RP-87-006 | C に `S(0.65, 10)` と `S(0.20, 4)` | C の threshold を 0.65 にした build が声と読む値 | 確率 ≥ 0.80 | C の知らせが 0 件（specified、RP-87 の (1) と陰性欄。C の threshold を B と同じ 0.65 にした build は、ここで 1 件出して落ちる。005 は 1 つ下、この行は B の線） | 陰性 | `c_does_not_start_speech_at_the_b_threshold_of_0_65` |
| T-RP-87-007 | C に `S(0.95, 10)` と `S(0.65, 4)` | 終わりの線の値 | 確率 < 0.65 | C の知らせが 0 件（specified、RP-87 の (1)「0.65 ちょうどでは終わらない」） | 境界、陰性 | `c_does_not_end_speech_at_exactly_0_65` |
| T-RP-87-008 | C に `S(0.95, 10)` と `S(0.64, 4)` | 終わりの線の 1 つ下 | 同上 | C の知らせが 1 件（specified、同上。007 の陽性対照） | 境界、陽性 | `c_ends_speech_below_0_65` |
| T-RP-87-009 | 持った候補を置いて、D に `S(0.50, 10)` と `S(0.20, 4)` | D の始まりの線の値 | 確率 ≥ 0.50 | D の知らせが 1 件。持った候補が `held_silence` で確定する（specified、RP-87 の (2)「0.50 以上で声を始め」。等号側） | 境界、陽性 | `d_starts_speech_at_exactly_0_50` |
| T-RP-87-010 | 持った候補を置いて、D に `S(0.49, 10)` と `S(0.20, 4)` | 始まりの線の 1 つ下 | 同上 | D の知らせが 0 件。確定は 0 件で、持った候補は残る（specified、同上） | 境界、陰性 | `d_does_not_start_speech_at_0_49` |
| T-RP-87-011 | 持った候補を置いて、D に `S(0.95, 10)` と `S(0.35, 4)` | 終わりの線の値 | 確率 < 0.35 | D の知らせが 0 件。確定は 0 件（specified、RP-87 の (2)「0.35 未満が…知らせ」。0.35 ちょうどは未満でない） | 境界、陰性 | `d_does_not_end_speech_at_exactly_0_35` |
| T-RP-87-012 | 持った候補を置いて、D に `S(0.95, 10)` と `S(0.34, 4)` | 終わりの線の 1 つ下 | 同上 | D の知らせが 1 件。`held_silence` で確定する（specified、同上。011 の陽性対照） | 境界、陽性 | `d_ends_speech_below_0_35` |
| T-RP-87-013 | 持った候補を置いて、D に `S(0.95, 10)` と `S(0.20, 3)` | 低い window が 3 個 | 低い window が 4 個 | D の知らせが 0 件。確定は 0 件（specified、RP-87 の (2)「0.12 秒続いた所」。002 と同じ向き） | 境界、陰性 | `d_does_not_notify_after_three_low_windows` |
| T-RP-87-014 | 持った候補を置かずに、D に `S(0.95, 10)` と `S(0.20, 4)`。確定の規則に流して持った候補が無い状態で D の知らせを渡す | 持った候補が無い | 持った候補が在る時だけ | 確定は 0 件。持った候補の状態は空のまま。後の A の区切りは `Plan::Whole` で読まれる（specified、RP-87 の (2)「持った候補が無い時は何もしない」。012 の陽性対照） | 陰性 | `d_does_nothing_without_a_held_candidate` |
| T-RP-87-015 | 公開した `JA_CONNECTIVE` の語を test が直に読み、語ごとに `あす雨が降る` の後ろに語を付けて `。` で閉じた文(たとえば `あす雨が降るから。`)を持った候補にして、1 文ずつ D の知らせ(`S(0.95, 10)` と `S(0.20, 4)`)を渡す | 接続の形で終わる。公開した表の全部の語 | 接続の形と助詞では D で確定しない | 表の語数が 0 でなく、全部の文で確定は 0 件、持った候補は残り、後の A の区切りで解かれる。落ちたら語を名指す(specified、RP-87 の (3)「2 つの表の全部の語」と H127 の 3。表から語を落とした build は、その語で `held_silence` を出して落ちる。test が語を書き写さないので、表に語が足されればその語も試される) | 陰性 | `a_held_sentence_ending_in_any_connective_is_not_confirmed_by_d` |
| T-RP-87-016 | 015 と同じで、公開した `JA_PARTICLE` の語を直に読んだ文 | 助詞で終わる。公開した表の全部の語 | 同上 | 表の語数が 0 でなく、全部の文で確定は 0 件、持った候補は残る。落ちたら語を名指す（specified、同上。015 の接続の形と別の表の側） | 陰性 | `a_held_sentence_ending_in_any_particle_is_not_confirmed_by_d` |
| T-RP-87-017 | 015 と同じで、公開した `JA_FINAL` の語を直に読んだ文 | 表に無い、文の終わりの形。公開した表の全部の語 | D で確定する | 表の語数が 0 でなく、全部の文で `held_silence` が 1 件確定する。落ちたら語を名指す（specified、RP-87 の (3)「それ以外の末尾では `held_silence` で確定し」。接続の形と助詞だけを 015、016 で落とし、それ以外を通すことの陽性対照。末尾が表に無い語の `です` を含む） | 陽性 | `a_held_sentence_with_any_other_ending_is_confirmed_by_d` |
| T-RP-87-018 | 持った候補を `今日は晴れです。` にして、D に `S(0.50, 10)` と `S(0.20, 4)` | 設計の陽性の case。末尾が表に無い語の `です` | D で確定する | `held_silence` で 1 件確定する。`final` の text は `今日は晴れです。`（specified、RP-87 の陽性欄「末尾が表に無い語（「です」）の持った文が `held_silence` で確定する」。017 の語の 1 つを取り出した形） | 陽性 | `a_held_sentence_ending_in_desu_is_confirmed_by_held_silence` |
| T-RP-87-019 | 持った候補を 3 入力にして D の知らせを渡す。(a) `明日は雨ですが。`、(b) `雨ですけど？`、(c) `雨なので！` | 終わりに `。` `？` `！` が付く。印を除いてから末尾を見る | 印を除いた形の末尾 | 3 入力とも確定は 0 件（specified、3.19 の D「持った文の末尾（`。` `？` `！` を除いた形）」。印を除かずに末尾を見る build は、`。` で終わる文を表に当てられず、`held_silence` を出して落ちる） | 境界、陰性 | `the_marks_are_removed_before_the_tail_is_read` |
| T-RP-87-020 | 持った候補を `ところが今日は晴れです。` にして D の知らせを渡す | 接続の形が文の途中に在り、末尾でない | 末尾の語だけを見る | `held_silence` で 1 件確定する（specified、同上。部分一致で見る build は `が` で止めて落ちる。015 の対） | 境界、陽性 | `a_connective_inside_the_sentence_does_not_stop_d` |
| T-RP-87-021 | 持った範囲の終わりを sample 20480、D の知らせを window 70 の終わり（sample 36352）で渡す。持った候補は `今日は晴れです。` | 確定の時刻 | `t1_s` は持った範囲の終わり | 確定した `final` の `t1_s` が 20480。D の知らせの sample 36352 ではない（specified、3.19 の D「`t1_s` は持った範囲の終わりの sample」） | 境界、陽性 | `the_confirmed_final_ends_at_the_end_of_the_held_range` |
| T-RP-87-022 | 持った候補を置き、持った範囲の終わり（sample 20480）より後（sample 24576）に B が声の始まりを読んだ後、D の知らせを渡す | B が D の知らせの前に声を読んだ | B が声を読んでいなければ確定 | 確定は 0 件。持った候補は捨てられ、後の A の区切りは持った候補を使わずに読まれる（`Plan::Held` でない）（specified、RP-87 の (4)） | 陰性 | `a_held_sentence_is_discarded_when_b_read_speech_before_the_d_notice` |
| T-RP-87-023 | 022 の B の声の始まりを持った範囲の終わりより前（sample 16384）にする | B の声が持った範囲の中 | 持った範囲の終わりから後の声だけ | `held_silence` で 1 件確定する（specified、RP-87 の (4)「持った範囲の終わりから D の知らせまでに B が声の始まりを読んでいなければ」。022 の陽性対照。持った文自身の声は数えない） | 境界、陽性 | `speech_inside_the_held_range_does_not_discard_the_held_sentence` |
| T-RP-87-024 | 1.28 秒の声 `S(0.95, 40)` の後に `S(0.60, 4)`、さらに `S(0.20, 4)`。確定の規則には、完結した文 `今日は晴れです。` を読ませる | C の知らせが B の知らせより先 | 先に来た知らせで解く | 最初の C の知らせで文が `rule` が `early` で確定する。`asr.endpoint.decided` 相当の `notice` が `c`（specified、RP-87 の (5)「C の知らせが B より先に来れば C で解き、`notice` が `c`」。0.60 は C の終わりの線 0.65 未満で B の終わりの線 0.50 以上なので、C が先に出る） | 陽性 | `a_c_notice_before_b_resolves_the_sentence_with_notice_c` |
| T-RP-87-025 | 1.28 秒の声 `S(0.70, 40)` と `S(0.20, 4)`。完結した文 `今日は晴れです。` | 声が 0.80 に届かず C が始まらない | B の知らせで解く | `rule` が `early`、`notice` が `b`（specified、同上。C が先に来ない時は B のまま） | 陽性 | `a_b_only_notice_resolves_the_sentence_with_notice_b` |
| T-RP-87-026 | 持った候補を置いて D の知らせで確定する（018 の形） | D で確定した発話の `notice` | `none` | `rule` が `held_silence`、`notice` が `none`（specified、3.19 の F86S の `held_silence` の行と RP-86 の (5)。D は `notice` の `c` でも `b` でもない） | 陽性 | `a_held_silence_decision_has_notice_none` |
| T-RP-87-027 | 024 の C の知らせで確定した後に、`S(0.70, 6)` と `S(0.20, 4)` を流す（B は 0.65 以上で声を始め、C は 0.80 未満で始めない） | 確定の線の後の声の続き | 声の続きの読みは B の物だけ | 声の続きが「在る」と記録される（B の声の始まりを `note_voice` に渡した形）。後の A の区切りは `Plan::Rest` になる（specified、RP-87 の (5)「声の続きの読みは B の物だけを使う」。C の始まりを声の続きに使う build は、0.70 で C が始まらないので「無い」と読み、`Plan::Drop` にして落ちる） | 陽性、陰性 | `speech_continuing_after_a_c_notice_is_read_by_b_only` |
| T-RP-87-028 | 024 の列そのもの（C の知らせの後に、B の知らせが来る） | C で確定した後に B が後から知らせる | 確定の線の規則に従う | B の知らせは新しい確定を作らない。確定は 1 件のまま（specified、3.19 の C「B が後から知らせても、C の候補で確定した後なら、今の確定の線の規則（3.17 の方針 4）に従う」。実装に当てて確かめていない、NOT VERIFIED、H129） | 境界、陰性 | `a_late_b_notice_after_a_c_confirmation_makes_no_second_final` |
| T-RP-87-029 | `--early-extra off` に当たる設定で検出器を開き、001 の列、009 の列、024 の列を流す。モードは `Mode::Early` | C と D が開かない | C と D は開かない | C の知らせが 0 件、D の知らせが 0 件、`held_silence` が 0 件。B は開いていて、001 の列で B の知らせが 1 件（specified、RP-87 の (6)。B が開いていることが、C と D の 0 件を「検出器が全部止まっている」と読み違えない対照。`--early-extra off` でも C を開く build は C の知らせを出して落ちる） | 陰性 | `early_extra_off_opens_neither_c_nor_d` |
| T-RP-87-030 | 029 と同じ設定の `--early-extra on` の形で、001 の列と 009 の列を流す。モードは `Mode::Early` | 開いた時 | C と D が開く | C の知らせが 1 件、D の知らせが 1 件（specified、RP-87 の (6)。029 の陽性対照。既定が `on`） | 陽性 | `early_extra_on_opens_c_and_d` |
| T-RP-87-031 | `Mode::Silence`（`--endpoint silence` に当たる）で、001 の列と 009 の列と 024 の列を流す。それぞれの後ろに、A の区切りが出る `S(0.20, 12)` を足す | A だけが開く | B、C、D は開かない | B の知らせ、C の知らせ、D の知らせがどれも 0 件。A の区切りは `silence` の規則で出る（specified、RP-87 の (6)、3.19 の「`--endpoint silence` は今のとおり B、C、D のどれも開かない」。`--early-extra on` の設定でも開かない） | 陰性 | `silence_mode_opens_none_of_b_c_and_d` |
| T-RP-87-032 | 検出器を開いて 400 window 流した後に `Vad::rebuild` を、A の threshold を 0.60 に替えた設定で呼び、001 の列、009 の列を流す | 4 本が同じ時に作り直される | 4 本の位置が同じ時計 | A、B、C、D の 4 本とも、再構築の後の最初の window を sample 400 × 512 として数え、知らせの位置が同じ基準で並ぶ（specified、RP-87 の (7)「`Vad::rebuild` が 4 本を同じ時に作り直す」。A と B だけを作り直す build は C と D の位置が 0 から数えた値になって落ちる） | 陽性、陰性 | `rebuild_resets_all_four_detectors_at_the_same_offset` |
| T-RP-87-033 | 032 の再構築の後に 004、005、009、010 の列を流す | 作り直した C と D が、A の設定を引き継がない | C は 0.80 と 0.65、D は 0.50 と 0.35 | 004 と 009 は知らせが出て、005 と 010 は出ない（specified、RP-87 の (7) と (1)(2)。A の threshold 0.60 に引かれて C や D の線が動く build は落ちる） | 境界、陰性 | `rebuild_keeps_the_c_and_d_lines_independent_of_the_main_threshold` |
| T-RP-87-034 | 検出器を開かずに、C と D の定数（始まりの線、終わりの線、`min_silence`）を公開の口から読む | 値の宣言 | C は 0.80、0.65、0.12 秒。D は 0.50、0.35、0.12 秒 | 6 つの値が上の通り（specified、RP-87 の (1)(2)。値を 1 か所の定数に持つことの確かめ。001 から 013 の列は、この定数と同じ値を前提にする） | 陽性 | `the_c_and_d_lines_are_declared_in_one_place` |
| T-RP-87-035 | 受入：018、004、009、005、006、015、016、029 の入力を順に流す | 全体 | 陽性 3 つは成立、陰性 5 つは出ない | 018 は `held_silence` で確定し、004 は C の知らせが 1 件、009 は D の知らせが 1 件。005 と 006 は C の知らせが 0 件、015 と 016 は確定が 0 件、029 は C と D の知らせが 0 件（specified、RP-87 の陽性欄「末尾が表に無い語（「です」）の持った文が `held_silence` で確定する。C の 0.80 ちょうどで声が始まる」と陰性欄の 3 つ。C の threshold を 0.65 にした build は 005 と 006 で、D を接続の形と助詞にも使う build は 015 と 016 で、`--early-extra off` でも C を開く build は 029 で落ちる） | 陽性、陰性 | `rp87_c_and_d_fire_on_their_own_lines_and_d_never_ends_a_connective_or_particle_tail`（未実装、持ち主 R8x-6） |
| T-RP-87-036 | 022 の B の声の始まりを、持った範囲の終わりちょうど(sample 20480)にして、D の知らせを渡す | B の声の始まりが持った範囲の終わりと一致する | 範囲の終わり以降の声は読んだ扱い | 確定は 0 件。持った候補は捨てられ、後の A の区切りは `Plan::Held` でない(specified、H127 の 6「B の声の始まりが持った範囲の終わりちょうどの時は B が声を読んだ扱いにし、持った文を捨てる」。範囲の終わりより後だけを読んだ扱いにする build は、`held_silence` を出して落ちる) | 境界、陰性 | `b_speech_starting_exactly_at_the_end_of_the_held_range_discards_the_held_sentence` |
| T-RP-87-037 | 022 の B の声の始まりを、持った範囲の終わりの 1 sample 手前(sample 20479)にして、D の知らせを渡す | 036 の 1 つ手前 | 同上 | `held_silence` で 1 件確定する(derived、H127 の 6 の線は「持った範囲の終わりから」。1 sample 手前は範囲の中なので読んだ扱いにしない。036 の対) | 境界、陽性 | `b_speech_one_sample_before_the_end_of_the_held_range_does_not_discard_it` |
| T-RP-87-038 | 1.28 秒の声 `S(0.95, 40)` の後に `S(0.20, 4)`。完結した文 `今日は晴れです。`。C と B が同じ window(声の後の 4 個目の低い window)で知らせる | C と B が同じ window | C の window が B の window より厳密に前の時だけ `c` | `rule` が `early`、`notice` が `b`(specified、H127 の 5「同じ window なら `b`」。同じ window を `c` と読む build はここで落ちる) | 境界、陰性 | `a_c_notice_in_the_same_window_as_b_resolves_with_notice_b` |
| T-RP-87-039 | 1.28 秒の声 `S(0.95, 40)` の後に `S(0.60, 1)` と `S(0.20, 4)`。完結した文 `今日は晴れです。`。C は 0.60 を含む 4 個目(声の後の 4 個目)で、B はその次の window で知らせる | C が B より厳密に 1 window 前 | 同上 | `rule` が `early`、`notice` が `c`(specified、H127 の 5「厳密に前の時だけ `c`」。038 の 1 window 後ろの側の対。ずらしが 1 window でも `c` になることを固定する) | 境界、陽性 | `a_c_notice_one_window_before_b_resolves_with_notice_c` |

**変異の確かめ**（検証の席が手で打つ。test の行ではない。設計の陰性欄）：

- C の threshold を 0.65 にした build で、005 と 006 が落ちる。
- D を接続の形と助詞の末尾にも使う build で、015 と 016 が落ちる。
- `--early-extra off` でも C を開く build で、029 が落ちる。
- 持った文の末尾を `。` `？` `！` を除かずに見る build で、019 が落ちる。
- `Vad::rebuild` が A と B だけを作り直す build で、032 が落ちる。

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- 確率を直接受ける口の形は H126(R8x-6 の席が決める。決めるまで、この節の行は打てない)。
- T-RP-87-028 の oracle は実装に当てていない(H129。検証の席が確かめる)。
- 知らせの window の数え方の細部は記録だけ(H127 の 4)。001 と 002 は 4 個と 3 個の両側だけを固定する。

### 4.80 RP-79（呼ぶことの確かめの 1 行。止めた）

**止めた。** 方針 B を外した。`rp79.rs` は作らない。H130。この節の行は実装されず、打たれない。
この節は、RP-79 の行のうち、E2E の run の門を呼ぶ確かめの 1 行だけを置いていた。RP-79 のほかの行は、この文書に無く、依頼もしない(H125)。
test：`native/hayamimi-eval/tests/rp79.rs`。受入 test の名前は `rp79_widened_early_end_shortens_ja_wait_without_mid_sentence_splits`。この節の行は fixture を読み、`--ignored` を付けずに通る。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp79`。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。設計は D 3.19 の「E2E の run の門」の 5 と、D 6 の RP-79 である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-79-001 | S、W、S、W の 4 run（ja の `s1.e2e1`、`w1.e2e1`、`s2.e2e1`、`w2.e2e1`。Rust の run で、それぞれ T-RP-83-001 の形の `<run>.machine.jsonl` を持つ）の dir で、`w2.e2e1` の `<run>.machine.jsonl` の行 1 の `load15` を `"2.50"` にする（止めた。方針 B を外した。`rp79.rs` は作らない。H130） | E2E の run の門を呼ぶ確かめ（RP-83 の線の値） | `load15` < 2.50 | `Err` で `w2.e2e1`、`load15`、`2.50` を名指す。ほかの 3 run の名前は名指さない（specified、3.19 の「E2E の run の門」の 5「呼ぶことの確かめ」。判定の関数が RP-83 の support を呼び忘れた test は、ここで落ちる。4 run 全部が内側の形が陽性対照。fixture の dir は門の前の記録の一覧に当たらない一時 dir） | 陰性 | `a_run_with_a_first_row_load15_of_2_50_is_rejected_by_the_gate` |

### 4.81 RP-57（DER の内訳）

test：`native/hayamimi-eval/src/der.rs`。受入 test の名前は `der_breakdown_isolates_confusion_from_miss`。
行は `der.rs` の `mod tests` に置く。全行が fixture を test の中に持ち、`--ignored` を付けずに通る。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --lib der_breakdown`。
走る行の数：同じ command に `-- --list 2>/dev/null | grep -c ': test$'` を足すと 19（行の数と同じ）。今の木では 0 で、`native` の `.rs` に `der_breakdown` の語は 0 件、陽性対照の `fn der` は `der.rs` に 1 件在る。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。内訳を返す関数の名前と引数の形、内訳の struct の欄の名前と型は実装の席が決める。行では Miss、FA、Confusion と呼ぶ。`check_breakdown_sum` と `check_isolation` は test の中の検査の関数で、016 から 019 がこの 2 つに陽性と陰性の対照を当てる（`check_isolation` の陽性の対照は 003 の本物の内訳）。
設計は D 8 の R12q と D 6 の RP-57 である。

**数え方**：

- 内訳は `der` と同じ分母の比である。分母は、collar の内側を除いた後の ref の発話時間の和で、重なった発話は話者の数だけ数える。Miss、FA、Confusion を足すと `der` の値になる。
- 定義は NIST の md-eval（SCTK）の `MISSED_SPEAKER`、`FALARM_SPEAKER`、`SPEAKER_ERROR` である。区間ごとに、ref の話者数 Nref、hyp の話者数 Nhyp、対応のついた話者数 Ncorrect を取る。Miss は max(Nref − Nhyp, 0)、FA は max(Nhyp − Nref, 0)、Confusion は min(Nref, Nhyp) − Ncorrect を、区間の長さで足した値である。対応は `der` と同じで、ファイル全体で重なりの和が最大になる 1 対 1 である。
- 3 つの和は、max(Nref, Nhyp) の積分から対応の重なりを引いた値になり、`der` の式（`load_length` から `best_pairing_overlap` を引く）と同じである。
- 各行の値は手で当て、同じ定義を 1 ms の格子で数える別の式（awk。対応は全順列の総当たり）で確かめた（2026-10-22。15 本の fixture で、3 つの比が 6 桁まで一致した）。
- 比べる許容は 1e-9 とする。境界の標本は 0.99e-9 と 1.01e-9 に置く。1e-9 そのものは f64 の丸めで内外が決まらず、0.6 の近くの f64 の刻みは 1.1e-16 なので、1% の差は区別できる。
- 内訳が collar をどう扱うかは受入の行に無い。`der` の分母と同じ時間で数える読みにした（011、014）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-57-001 | ref = A 0〜5、B 5〜10。hyp = ref と同じ（collar 0） | 一致 | 3 つとも 0 | Miss、FA、Confusion が 3 つとも 0（specified、RP-57 の「同じ ref と hyp で全部 0」。格子の式でも 0） | 陽性 | `der_breakdown_zero_for_identical_ref_and_hyp` |
| T-RP-57-002 | ref は 001 と同じ。hyp = S2 0〜5、S1 5〜10（名前だけ違う） | 名前でなく対応で決まる | 3 つとも 0 | 3 つとも 0（derived、`der.rs` の既存の test `names_do_not_matter_only_the_pairing` と同じ入力を内訳で読む）。名前の一致を数える build は全区間が不一致になり、Confusion が 1.0 になって落ちる | 境界、陽性 | `der_breakdown_ignores_speaker_names_when_pairing` |
| T-RP-57-003 | 受入：2 入力（collar 0）。(a) ref = A 0〜10、hyp = A 0〜5、B 5〜10（後ろの半分を別の話者にした）。(b) ref は同じ、hyp = A 0〜5（後ろの半分を落とした） | 入れ替えは Confusion だけ、落とすと Miss だけ | 各比の誤差 1e-9 以内 | (a) は Confusion 0.5、Miss 0、FA 0。(b) は Miss 0.5、Confusion 0、FA 0。どちらも 3 つの和が `der` の値 0.5 と 1e-9 以内で同じ（specified、RP-57。pseudo：格子の式が (a) で confusion=0.500000、miss=0、fa=0、(b) で miss=0.500000、confusion=0、fa=0）。Confusion を Miss に足す内訳は、(a) の Miss が 0.5、Confusion が 0 になって落ちる | 陽性、陰性 | `der_breakdown_isolates_confusion_from_miss` |
| T-RP-57-004 | ref = A 0〜4、B 4〜10。hyp = X 0〜6、Y 6〜10（話者の交代が 2 秒遅れ。collar 0） | 交代の位置のずれ | 各比の誤差 1e-9 以内 | Confusion 0.2、Miss 0、FA 0（derived。対応は A と X、B と Y で合う時間が 4 + 4 = 8、load が 10 なので 2 / 10。4〜6 の ref B に hyp X が当たり、人数は 1 対 1 なので Miss にも FA にもならない。pseudo：格子の式で confusion=0.200000） | 境界、陽性 | `der_breakdown_counts_a_late_speaker_change_as_confusion` |
| T-RP-57-005 | ref = A 0〜5、B 5〜10。hyp = A 0〜5（2 人目を丸ごと落とした。collar 0） | hyp の人数が足りない | 各比の誤差 1e-9 以内 | Miss 0.5、FA 0、Confusion 0（derived。5〜10 は Nref 1、Nhyp 0。pseudo：格子の式で miss=0.500000） | 陽性 | `der_breakdown_counts_a_dropped_speaker_as_miss` |
| T-RP-57-006 | ref = A 0〜10。hyp = A 0〜10、A 12〜14（ref の外に hyp の発話が在る。collar 0） | ref の無い時間の hyp | 各比の誤差 1e-9 以内 | FA 0.2、Miss 0、Confusion 0（derived。12〜14 は Nref 0、Nhyp 1 で 2 / 10。pseudo：格子の式で fa=0.200000） | 陽性 | `der_breakdown_counts_speech_outside_the_reference_as_false_alarm` |
| T-RP-57-007 | ref = A 0〜10。hyp = A 0〜10、B 0〜5（ref の発話の中で hyp の人数が多い。collar 0） | ref の中で Nhyp が Nref を超える | 各比の誤差 1e-9 以内 | FA 0.5、Miss 0、Confusion 0（derived。0〜5 は Nref 1、Nhyp 2 で、A が A に対応して B が余る。pseudo：格子の式で fa=0.500000） | 陽性 | `der_breakdown_counts_an_extra_hypothesis_speaker_as_false_alarm` |
| T-RP-57-008 | ref = A 0〜10、B 5〜10（5〜10 は 2 人の重なり）。hyp = A 0〜10（collar 0） | ref の重なりに hyp が答えない | 各比の誤差 1e-9 以内 | Miss 1/3、FA 0、Confusion 0（derived。分母は 10 + 5 = 15 秒で、5〜10 は Nref 2、Nhyp 1 なので Miss が 5 秒。pseudo：格子の式で miss=0.333333） | 境界、陽性 | `der_breakdown_counts_unanswered_overlap_as_miss` |
| T-RP-57-009 | ref = A 0〜5、B 5〜10。hyp = X 0〜10（2 人を 1 人にまとめた。collar 0） | hyp の話者が少ないが、人数は 1 対 1 | 各比の誤差 1e-9 以内 | Confusion 0.5、Miss 0、FA 0（derived。X は A か B のどちらか 1 人にだけ対応し、残りの 5 秒が Confusion。pseudo：格子の式で confusion=0.500000） | 陽性 | `der_breakdown_counts_merged_speakers_as_confusion` |
| T-RP-57-010 | ref = A 0〜10、B 10〜20。hyp = X 0〜8、Y 8〜14、X 16〜24（collar 0） | 3 つが 1 つの入力に同時に出る | 各比の誤差 1e-9 以内 | Miss 0.1、FA 0.2、Confusion 0.3、和 0.6（derived。採点する時間は 20 秒。対応は A と X、B と Y。0〜8 と 10〜14 は一致、8〜10 は Confusion 2、14〜16 は Miss 2、16〜20 は Confusion 4、20〜24 は FA 4。pseudo：格子の式で miss=0.100000、fa=0.200000、confusion=0.300000、der=0.600000） | 陽性 | `der_breakdown_separates_miss_false_alarm_and_confusion_in_one_input` |
| T-RP-57-011 | collar 0.25。ref = A 0〜10、B 10〜20。hyp = A 0〜10.5、B 10.5〜20 | collar の外に出るずれ | 各比の誤差 1e-9 以内 | Confusion 0.25 / 19、Miss 0、FA 0（derived。除いた後の ref は A 0.25〜9.75、B 10.25〜19.75 で 19 秒。hyp の A の 10.25〜10.5 が ref の B に当たり、0.25 秒が Confusion。和は `der` の collar 0.25 の値と同じ。pseudo：格子の式で confusion=0.013158） | 境界、陽性 | `der_breakdown_scores_only_outside_the_collar` |
| T-RP-57-012 | ref = なし（0 本）。hyp = A 0〜1（collar 0） | ref の発話時間が 0 | 3 つとも 0 で有限 | 3 つとも 0 で、NaN でない（specified、`der` の doc「A reference with no speech time scores 0」と RP-57 の「和が `der` の値と同じ」）。0 を 0 で割る build は NaN を返し、`der` の 0 との差が 1e-9 以内にならず落ちる | 境界、陰性 | `der_breakdown_of_an_empty_reference_is_zero_not_nan` |
| T-RP-57-013 | ref = A 0〜3。hyp = なし（0 本）。collar 0 | hyp が空 | 各比の誤差 1e-9 以内 | Miss 1.0、FA 0、Confusion 0（derived。`der.rs` の既存の test `an_empty_hypothesis_misses_everything` の入力を内訳で読む） | 境界、陽性 | `der_breakdown_of_an_empty_hypothesis_is_all_miss` |
| T-RP-57-014 | collar 0.25。ref = A 0〜0.4。hyp = B 0〜0.4 | collar が ref の時間を全部除く | 3 つとも 0 で有限 | 3 つとも 0 で、NaN でない（derived。除く区間は −0.25〜0.25 と 0.15〜0.65 で、つないで 0〜0.4 を覆い、分母が 0 になる。`der` は 0 を返す。pseudo：格子の式で total=0） | 境界、陰性 | `der_breakdown_is_zero_when_the_collar_covers_all_reference_time` |
| T-RP-57-015 | 001 から 014 の fixture の全部を 1 本の test で順に読む（003 の 2 入力と、collar 0.25 の 2 本を含む） | 性質：和が `der` | 全入力で、和と `der` の差の絶対値が 1e-9 以下 | 全部で成り立つ（specified、RP-57 の「3 つの和が `der` の値と 1e-9 以内で同じ」。`der` は同じ入力で別に呼んだ値で、内訳の中身から出さない） | 陽性、性質 | `der_breakdown_sums_to_der_on_every_fixture` |
| T-RP-57-016 | `check_breakdown_sum` に、`der` 0.6 と、Miss 0.1、FA 0.2、Confusion 0.3 に 0.99e-9 を足した内訳 | 1e-9 の内側 | `Ok` | `Ok`（specified、RP-57 の「1e-9 以内」。1% 内側の標本） | 境界、陽性 | `der_breakdown_sum_check_accepts_a_difference_just_inside_1e_9` |
| T-RP-57-017 | 016 の内訳に足す値を 1.01e-9 にする | 1e-9 の外側 | `Err` | `Err`（specified、同上。1% 外側の標本。許容を 1e-8 などに広げた検査は、ここで落ちない build を通すので、この行で落とす） | 境界、陰性 | `der_breakdown_sum_check_rejects_a_difference_just_outside_1e_9` |
| T-RP-57-018 | `check_breakdown_sum` に、`der` 0.5 と、Confusion を数えない内訳（Miss 0、FA 0、Confusion 0）。003 の (a) の `der` | 和が足りない内訳 | `Err` | `Err`（specified、RP-57 の「和が `der` の値と同じ」。Confusion を落とした build は和が 0 で、`der` の 0.5 と違う） | 陰性 | `der_breakdown_sum_check_rejects_a_dropped_confusion` |
| T-RP-57-019 | `check_isolation` に、003 の (a) の内訳として Miss 0.5、FA 0、Confusion 0（Confusion を Miss に足した値） | Confusion が Miss に混ざった内訳 | `Err` | `Err` で、Miss が 0 でないことを名指す（specified、RP-57 の陰性欄「Confusion を Miss に足す内訳で、入れ替えの行が落ちる」。和は 0.5 で `der` と合うので 018 の検査は通る。003 の隔離の検査だけがこの内訳を落とす） | 陰性 | `der_breakdown_isolation_check_rejects_confusion_folded_into_miss` |

### 4.82 RP-58（Windows の COM の初期化の HRESULT を分ける）

test：`native/hayamimi/src/capture/windows.rs`、`native/hayamimi/src/capture/com_scope.rs`。受入 test の名前は `windows_com_scope_uses_existing_changed_mode_apartment`。
行は 2 つの置き場に分かれる。001 から 010 は、HRESULT を分ける純関数（001 から 006）と scope の型（007 から 010。偽の後始末を渡す）の行で、cfg を外した module（`com_scope.rs`）の `mod tests` に置き、Windows でなくても走る。受入の名前の 008 も `com_scope.rs` に置く（rp82 は 2 file の和で名前を引く）。011 と 012 は本物の COM を呼ぶ Windows の実走の行で、`windows.rs` の `mod tests` に置く。この module は Windows の build でだけ組まれるので、`#[ignore]` は付けない。
command：001 から 010 は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi --lib windows_com_scope`（WSL）。011 と 012 は同じ command を Windows の shell で打つ（`flock` は無い）。
走る行の数：WSL で同じ command に `-- --list 2>/dev/null | grep -c ': test$'` を足すと 10、Windows で 12 になる。陽性対照は、同じ list に `windows_com_scope_classifies_s_ok_as_needing_cleanup` が 1 件在ること。WSL の 10 は、純関数と scope の型の行が cfg を外した module に在ることを示す。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。HRESULT を分ける純関数（HRESULT と endpoint の名前を受け、後始末の要る初期化、後始末しない既存の初期化、`CaptureError` のどれかを返す）の名前と引数の形、scope の型の名前は実装の席が決める。scope の型は後始末の関数（`FnMut()` か小さな trait）を引数に取る。007 から 010 は、呼ばれた回数を数える偽の関数を渡す。
設計は D 3.7 の COM の初期化、D 8 の R12r、D 6 の RP-58 である。

**数え方**：

- HRESULT は符号つき 32 bit の整数である。値は Microsoft の文書の値を literal で test に書く。S_OK は 0、S_FALSE は 1、RPC_E_CHANGED_MODE は 0x80010106、E_FAIL は 0x80004005、E_INVALIDARG は 0x80070057、E_OUTOFMEMORY は 0x8007000E、E_UNEXPECTED は 0x8000FFFF である（`CoInitializeEx` の文書と COM のエラー番号の表を 2026-10-22 に読んだ）。`windows` と `wasapi` の crate の定数を test で使わない。実装が同じ定数を使うと、定数の打ち間違いが自分と一致するからである。
- 成功は S_OK と S_FALSE の 2 つだけとする。設計の文を字義どおりに読み、`SUCCEEDED` の「0 以上」にはしない。`CoInitializeEx` は 2 つ以外の成功を返さないので、2 などは未知の値として失敗に分ける（005）。
- Microsoft の文書は、S_FALSE を返した呼び出しも `CoUninitialize` と対にすると書く。S_FALSE は S_OK と同じく後始末が要る。RPC_E_CHANGED_MODE は初期化が成立していないので、後始末を呼ばない。
- endpoint の名前は呼び出し側が渡す文字列である。test は `mic-probe` と `speaker-probe` を渡し、実装の既定の名前に依存しない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-58-001 | HRESULT 0（S_OK）、endpoint の名前 `mic-probe` | 成功の 1 つ目 | 後始末の要る初期化 | 後始末の要る初期化を返し、`Err` でない（specified、RP-58。rp19 の `windows_com_scope_balances_every_success_result` の 0 の側） | 陽性 | `windows_com_scope_classifies_s_ok_as_needing_cleanup` |
| T-RP-58-002 | HRESULT 1（S_FALSE）、endpoint の名前 `mic-probe` | 成功の 2 つ目（同じ thread の 2 度目の呼び出し） | 後始末の要る初期化 | 後始末の要る初期化を返す（specified、RP-58 と Microsoft の文書の「S_FALSE を返した呼び出しも `CoUninitialize` と対にする」）。S_FALSE を既存の初期化に分けて後始末を落とす build は、ここで落ちる | 境界、陰性 | `windows_com_scope_classifies_s_false_as_needing_cleanup` |
| T-RP-58-003 | HRESULT 0x80010106（RPC_E_CHANGED_MODE）、endpoint の名前 `mic-probe` | 既存の別 mode の apartment | 後始末しない既存の初期化 | 後始末しない既存の初期化を返し、`Err` でない（specified、RP-58）。今の `run`（`initialize_mta().ok().map_err(device_error)?`）の形は `Err` を返すので、この行で落ちる | 陰性 | `windows_com_scope_classifies_changed_mode_as_an_existing_apartment` |
| T-RP-58-004 | HRESULT 0x80004005（E_FAIL）、0x80070057（E_INVALIDARG）、0x8007000E（E_OUTOFMEMORY）、0x8000FFFF（E_UNEXPECTED）の 4 つ。endpoint の名前 `mic-probe` | 失敗の代表 | `Err` | 4 つとも `CaptureError` の `Err`（specified、RP-58 の「それ以外」）。どれも後始末の要る初期化にも既存の初期化にも分けない | 陰性 | `windows_com_scope_classifies_other_failures_as_errors` |
| T-RP-58-005 | HRESULT 2、-1（0xFFFFFFFF）、0x80010105、0x80010107。endpoint の名前 `mic-probe` | 名指しした値の隣。成功の 1 の隣（2）、負の端（-1）、RPC_E_CHANGED_MODE の前後 1 | `Err` | 4 つとも `Err`（specified、RP-58 の「それ以外」）。上位の bit や符号だけで分ける build は、0x80010105 と 0x80010107 を既存の初期化に分けて落ちる。0 以上を成功とする build は 2 を成功に分けて落ちる | 境界、陰性 | `windows_com_scope_values_next_to_the_named_ones_are_errors` |
| T-RP-58-006 | HRESULT 0x80004005 を、endpoint の名前 `mic-probe` と `speaker-probe` で 1 回ずつ。HRESULT 0x8007000E を `mic-probe` で 1 回 | 名前と HRESULT の値が結果に出る | 渡した名前と、渡した HRESULT の 16 進 | それぞれの `Err` が、渡した名前と、渡した HRESULT の `0x80004005`、`0x8007000E` の形の 16 進（大文字、8 桁）の両方を文言に持ち、もう片方の名前と値を持たない（specified、RP-58 の「endpoint の名前を持つ `CaptureError`」と評議の決め）。名前や値を固定の文字列にする build は、どれかで落ちる。文言は `to_string()` で見る | 陰性 | `windows_com_scope_error_carries_the_endpoint_name_it_was_given` |
| T-RP-58-007 | 偽の後始末（呼んだ回数を数える）。HRESULT 0 と 1 のそれぞれで scope を作り、scope が生きている間と、落とした後で回数を読む | 成功の 2 つ | 後始末は 1 回 | どちらも、生きている間は 0 回、落とした後は 1 回（specified、RP-58 の「成功の時だけ後始末を 1 回呼ぶ」）。作った時に呼ぶ build と、2 回呼ぶ build は落ちる | 陽性 | `windows_com_scope_balances_every_success_result` |
| T-RP-58-008 | 受入：偽の後始末。HRESULT 0x80010106 で scope を作り、本体を走らせ、落とした後で回数を読む | 既存の別 mode の apartment | 本体が走る。後始末は 0 回 | scope が作れて（`Ok`）、本体が走り、後始末は 0 回（specified、RP-58）。今の `run` の形は scope が作れず、本体が走らないので落ちる。003 の純関数の行の scope 側の対 | 陰性 | `windows_com_scope_uses_existing_changed_mode_apartment` |
| T-RP-58-009 | 偽の後始末。HRESULT 0x80004005 で scope を作る | 失敗 | scope が無い | `Err` で、本体が走らず、後始末は 0 回（specified、RP-58。失敗した初期化の後で本体に入ると、COM が無いまま capture が始まる） | 陰性 | `windows_com_scope_rejects_other_hresult_failures` |
| T-RP-58-010 | 偽の後始末。HRESULT 0 で scope を作り、本体が `Err` を返す | 本体の失敗 | 後始末は 1 回 | 本体の `Err` がそのまま返り、後始末は 1 回（specified、RP-58。今の `run` が capture の結果を持って `deinitialize` を呼ぶ形を保つ） | 境界 | `windows_com_scope_cleans_up_once_when_the_body_fails` |
| T-RP-58-011 | 実走：Windows の新しい thread で、本物の初期化（`initialize_mta`）で scope を作って落とし、続けて同じ thread でもう一度 `initialize_mta` を呼ぶ | 本物の COM で後始末が済む | 2 度目が S_OK | 2 度目の戻りが S_OK（0）で、S_FALSE（1）でない（derived、Microsoft の文書：初期化済みの thread への 2 度目の同じ mode の呼び出しは S_FALSE を返し、後始末が済んでいれば再び S_OK を返す）。後始末が漏れた build は 2 度目が S_FALSE になって落ちる。2 度目の呼び出しも `deinitialize` で対にする | 陽性 | `windows_com_scope_leaves_a_real_thread_uninitialized_after_the_scope` |
| T-RP-58-012 | 実走：Windows の新しい thread を `initialize_sta` で STA にしておき、その thread で本物の scope を作って（MTA を求める）落とし、続けて同じ thread で `initialize_sta` をもう一度呼ぶ | 本物の RPC_E_CHANGED_MODE | scope が `Ok`。後始末は呼ばれない | scope が `Ok` で、落とした後の `initialize_sta` が S_FALSE（1）を返す（derived、Microsoft の文書：STA が初期化されたままなら 2 度目は S_FALSE）。既存の STA に後始末を呼ぶ build は STA を解いてしまい、2 度目が S_OK になって落ちる。呼んだ分は `deinitialize` で対にする | 陰性 | `windows_com_scope_keeps_an_existing_sta_apartment_of_a_real_thread` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

- `speaker_capture_failure_is_reported_with_endpoint_context` は RP-19 の対応表が持つ。この節に行は置かない。
- 呼び出し側（`windows.rs` の `run`）が実際の endpoint の名前を渡して、`supervise.rs` が log に出すこと。Windows の実機の run でだけ測れる。持ち主は T-RP-10-007 の `windows_loopback_reaches_events`（R13-3）である。

### 4.83 RP-60（2 人の話者の材料）

test：`native/hayamimi-eval/src/two_speakers.rs` の `mod tests`（001 から 035）と、`native/hayamimi-eval/src/cli.rs` の `mod tests`（036 だけ）。受入 test の名前は `two_speaker_clip_is_cut_by_the_rule_from_ami`。
036 は subcommand の引数の parse から書き出しまでを通すので `cli.rs` に置く。ほかの行は `two_speakers.rs` に置く。027 だけが `testdata/eval_diar/`（git 管理外）を読み、`#[ignore]` を持つ。ほかの行は test の中で RTTM と wav を作る。scratch の dir は `testutil::scratch`（`target/eval-test-tmp/`）で取る。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --lib two_speaker_clip`。
走る行の数：同じ command に `-- --list 2>/dev/null | grep -c ': test$'` を足すと 36（行の数と同じ。`#[ignore]` の 027 も list に出る。036 は `cli.rs` の test で、名前が `two_speaker_clip` で始まるので同じ filter が拾う）。027 は `-- --ignored` を足して打つ。行が全部 実装された木では、`git grep -h 'fn two_speaker_clip_' -- native | wc -l` も 36 になる。陽性対照：`git grep -h 'fn two_speaker_clip_picks_the_window_of_a_hand_placed_rttm' -- native | wc -l` は 1（001 の関数）。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。窓を選ぶ関数（会議の RTTM の dir を受け、窓か `Err` を返す）と、wav と manifest を書く関数（窓と dir を受ける）の名前と引数の形、manifest の key の綴りは実装の席が決める。test は key の綴りを定数として 1 か所に持つ。
設計は D 8 の R12u1 と、その小節の「材料が無い 4 行」、D 6 の RP-60 である。

**窓の読み**（評議の決めと、設計の席の読み）：

- 窓は、始まりで並べた連続する 4 区間である。長さが 0 以下の区間は、窓を選ぶ前に捨てる（4 区間に数えず、`Err` にもしない）。窓の始まりは最初の区間の始まり、終わりは最後の区間の終わりとする。窓の外の区間が、窓の始まりから終わりまでと 1 ms 以上重なる窓は採らない。接するだけ（重なる時間が 0）なら採る（034、035）。窓の長さと、区間の間の無音には上限を置かない。
- 窓の中の話者がちょうど 2 人で、少ない方の話者の区間がちょうど 1 つ（もう 1 人は 3 つ）、その 1 区間の長さが 2.000 秒以上 4.000 秒以下である。「2〜4 秒」は両端を含む。
- 重なりは窓の 4 区間のどの 2 つの間でも 0 である。同じ話者の区間どうしも数える。時間の積が 1 ms 以上なら重なり、前の終わりと次の始まりが同じ値で接するだけ（積 0）なら重なりでない。
- 会議は RTTM の file の名前（拡張子を除く）の昇順の文字列で読み、窓が見つかった最初の会議の、始まりが最も早い窓を採る。
- RTTM の秒は小数 3 桁（1 ms）である。境界の標本は 1 ms 刻みで置く。f64 の足し引きで境界を 1 ms 未満でまたぐ値を入れた。(0.010 + 2.000) − 0.010 は 1.9999999999999998、(4.001 + 4.000) − 4.001 は 4.000000000000001、0.131 + 3.000 は 3.1310000000000002 で、次の区間の始まり 3.131（3.1309999999999998）より 4.4e-16 大きい。ms の整数で比べない build は、002、004、011 で落ちる。
- 窓の音声は、元の wav と同じ 16 kHz、16 bit、mono のまま、始まりの sample から（始まり × 16000）、終わりの sample の手前まで（終わり × 16000）を切る。ms 刻みなので sample は整数になる。manifest の秒は、ms の整数に読んで比べる。

**fixture**（RTTM の 1 行は `SPEAKER <会議> 1 <始まり> <長さ> <NA> <NA> <話者> <NA> <NA>`。表では「話者 始まり 長さ」と書き、単位は秒。会議の名前は mA と mB）：

- **F60v**（mA）：A 10.000 5.000、B 16.000 3.000、A 20.000 4.000、A 25.000 2.500。窓は 10.000 から 27.500。少ない方の B は 1 区間で 3.000 秒。
- **F60lo**（mA。下の境界）：B 0.010 2.000、A 3.000 1.000、A 5.000 1.000、A 7.000 1.000。窓は 0.010 から 8.000。
- **F60hi**（mA。上の境界）：B 4.001 4.000、A 9.000 1.000、A 11.000 1.000、A 13.000 1.000。窓は 4.001 から 14.000。
- **F60to**（mA。接する）：B 0.131 3.000、A 3.131 1.000、A 6.000 1.000、A 8.000 1.000。窓は 0.131 から 9.000。B の終わり 3.131 に A が始まる。
- **F60ov**（mA。重なりだけ）：A 10.000 5.000、B 14.000 3.000、A 20.000 4.000、A 25.000 2.500。A と B が 14.000 から 15.000 の 1.000 秒重なる。
- **S60**（音声）：16 kHz、16 bit、mono、30.000 秒（480000 sample）。sample i の値は、i の下位 16 bit を符号つきで読んだ値（i が 160000 で 28928、160001 で 28929、439999 で −18753、400000 で 6784、479999 で 21247）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-60-001 | F60v の 1 会議（mA） | 規則に合う窓が 1 つ。少ない方の区間が 2〜4 秒の内側（3.000） | 窓が 1 つ | 窓は mA の 10.000 から 27.500（specified、RP-60 の陽性「規則に合う窓を手で置いた RTTM の fixture から同じ窓を選ぶ」。pseudo：別の式（awk）で同じ fixture を数え、連続する 4 区間の候補が 1 つで 10.000 から 27.500） | 陽性 | `two_speaker_clip_picks_the_window_of_a_hand_placed_rttm` |
| T-RP-60-002 | F60lo の 1 会議 | 少ない方の区間 2.000 秒（下の境界の上の側）。f64 で 1.9999999999999998 になる始まり | 2.000 秒以上 | 窓は mA の 0.010 から 8.000（specified、RP-60 の「2〜4 秒」を両端を含む読み。ms の整数でなく f64 の差で比べる build は 1.9999999999999998 を 2 未満と読んで落ちる） | 境界、陽性 | `two_speaker_clip_accepts_a_minority_turn_of_exactly_2_seconds` |
| T-RP-60-003 | 002 の B の長さを 1.999 にする | 下の境界の下の側（1 ms 外） | 2.000 秒以上 | `Err` で、規則に合う窓が無いことを名指す（specified、同上）。002 が同じ fixture の陽性対照で、落ちる理由は長さだけ | 境界、陰性 | `two_speaker_clip_rejects_a_minority_turn_of_1_999_seconds` |
| T-RP-60-004 | F60hi の 1 会議 | 少ない方の区間 4.000 秒（上の境界の内側）。f64 で 4.000000000000001 になる始まり | 4.000 秒以下 | 窓は mA の 4.001 から 14.000（specified、同上。f64 の差で比べる build は 4.000000000000001 を 4 超と読んで落ちる） | 境界、陽性 | `two_speaker_clip_accepts_a_minority_turn_of_exactly_4_seconds` |
| T-RP-60-005 | 004 の B の長さを 4.001 にする | 上の境界の外側（1 ms 外） | 4.000 秒以下 | `Err` で、規則に合う窓が無いことを名指す（specified、同上。004 が陽性対照） | 境界、陰性 | `two_speaker_clip_rejects_a_minority_turn_of_4_001_seconds` |
| T-RP-60-006 | F60v の C を足した形：A 10.000 5.000、B 16.000 3.000、C 20.000 4.000、A 25.000 2.500 | 話者 3 人（区間は 4 つ） | 話者はちょうど 2 人 | `Err` で、規則に合う窓が無いことを名指す（specified、RP-60 の「窓の中の話者が 2 人」。001 が対の陽性） | 境界、陰性 | `two_speaker_clip_rejects_three_speakers` |
| T-RP-60-007 | A 10.000 5.000、A 16.000 3.000、A 20.000 4.000、A 25.000 2.500 | 話者 1 人（少ない方の話者が無い） | 話者はちょうど 2 人 | `Err` で、規則に合う窓が無いことを名指す（specified、同上） | 境界、陰性 | `two_speaker_clip_rejects_one_speaker` |
| T-RP-60-008 | A 10.000 5.000、B 16.000 3.000、A 20.000 4.000、B 25.000 3.000 | 2 人が 2 区間ずつ（少ない方の区間が 2 つ） | 少ない方の区間は 1 つ | `Err` で、規則に合う窓が無いことを名指す（specified、RP-60 の「少ない方の区間が 1 つ」。001 が対の陽性） | 境界、陰性 | `two_speaker_clip_rejects_two_turns_for_each_speaker` |
| T-RP-60-009 | A 10.000 5.000、A 16.000 3.000、A 20.000 4.000、B 25.000 3.000 | 少ない方の区間が最後の位置（001 は 2 番目、002 と 004 は先頭） | 位置によらない | 窓は mA の 10.000 から 28.000（specified、RP-60。位置を決め打つ build は落ちる） | 境界、陽性 | `two_speaker_clip_accepts_the_minority_turn_at_the_end` |
| T-RP-60-010 | A 10.000 5.000、B 16.000 3.000、A 20.000 4.000 | 区間が 3 つ（4 つに 1 足りない） | 区間は 4 つ | `Err` で、規則に合う窓が無いことを名指す（specified、RP-60 の「区間が 4 つ」。001 が 4 つの陽性。5 つの側は残る穴の 1） | 境界、陰性 | `two_speaker_clip_rejects_three_turns` |
| T-RP-60-011 | F60to の 1 会議 | 重なり 0 の境界の内側。B の終わりと A の始まりが同じ値（f64 で 3.1310000000000002 と 3.1309999999999998） | 重なり 0 | 窓は mA の 0.131 から 9.000（specified、RP-60 の「重なりが 0」。ms の整数でなく f64 の足し算で重なりを数える build は 4.4e-16 秒の重なりを見て落ちる） | 境界、陽性 | `two_speaker_clip_accepts_turns_that_touch_with_no_overlap` |
| T-RP-60-012 | 011 の 2 番目の区間を A 3.130 1.000 にする | 重なり 1 ms（境界の外側の最小） | 重なり 0 | `Err` で、規則に合う窓が無いことを名指す（specified、同上。011 が同じ fixture の陽性対照で、落ちる理由は 1 ms の重なりだけ） | 境界、陰性 | `two_speaker_clip_rejects_an_overlap_of_1_ms` |
| T-RP-60-013 | F60ov の 1 会議 | 重なりの在る窓だけ（1.000 秒） | 重なり 0 | `Err` で、規則に合う窓が無いことを名指す。`Ok` の窓を返さない（specified、RP-60 の陰性「重なりの在る窓だけの fixture で、窓が無いと名指しして `Err` を返す」。001 が同じ区間の長さの陽性） | 陰性 | `two_speaker_clip_names_the_missing_window_when_only_overlapping_windows_exist` |
| T-RP-60-014 | dir に mB.rttm（F60v）と mA.rttm（A 100.000 5.000、B 106.000 3.000、A 110.000 4.000、A 115.000 2.500）。書く順は mB、mA | 会議の名前の順と、窓の始まりの順が逆 | 名前の昇順で最初の会議 | 窓は mA の 100.000 から 117.500（specified、8 の小節「会議の名前の順に読み」。始まりが最も早い 10.000 の mB を採る build と、書いた順に読む build は落ちる） | 境界、陽性 | `two_speaker_clip_reads_the_meetings_in_name_order` |
| T-RP-60-015 | dir に mA.rttm（F60ov の区間）と mB.rttm（F60v の区間） | 先に読む会議に窓が無い | 窓の在る最初の会議 | 窓は mB の 10.000 から 27.500（specified、8 の小節の「最初の窓」。窓の無い会議で `Err` を返して止まる build は落ちる） | 境界、陽性 | `two_speaker_clip_skips_a_meeting_with_no_window` |
| T-RP-60-016 | mA の 1 会議。F60v の 4 行と、F60v を 90 秒ずらした 4 行（A 100.000 5.000、B 106.000 3.000、A 110.000 4.000、A 115.000 2.500）。file には後ろの 4 行を先に書き、F60v の 4 行は逆順で書く | 1 会議に窓が 2 つ。file の行が時刻順でない | 始まりが最も早い窓 | 窓は mA の 10.000 から 27.500（specified、8 の小節「最初の窓」。file の行の順に窓を探す build は 100.000 の窓を採って落ちる） | 境界、陽性 | `two_speaker_clip_takes_the_earliest_window_of_a_meeting` |
| T-RP-60-017 | RTTM が 1 つも無い dir（dir は在る） | 会議が 0 | 会議が在る | `Err` で、dir の path を名指す。「窓が無い」と同じ文言にしない（implicit、0 会議を通さない。013 の `Err` と別の理由であることを、path の名指しで分ける） | 境界、陰性 | `two_speaker_clip_rejects_a_directory_with_no_rttm` |
| T-RP-60-018 | 0 byte の RTTM が 1 つの dir（mA.rttm） | 区間が 0 | 区間が在る | `Err` で、規則に合う窓が無いことを名指す（implicit、空の入力を Ok の窓にしない） | 境界、陰性 | `two_speaker_clip_rejects_an_rttm_with_no_turns` |
| T-RP-60-019 | mA.rttm の 2 行目が `SPEAKER mA 1 12.0 1.0`（欄が足りない行）。1 行目は F60v の最初の行 | 壊れた RTTM | 壊れた行は読めない | `Err` で、`mA.rttm:2` を名指す。規則に合う窓が無いとは言わない（derived、`parse_rttm` の「malformed SPEAKER line は origin と行を名指す」を、窓を選ぶ側が握りつぶさない） | 境界、陰性 | `two_speaker_clip_reports_a_malformed_rttm_line_not_a_missing_window` |
| T-RP-60-020 | 窓 mA の 10.000 から 27.500。元の音声は S60（mA.wav として dir に置く） | 切り出し。始まりと終わりが sample の整数 | 長さと中身 | 出力の wav は 16 kHz、16 bit、mono で、280000 sample（(27.500 − 10.000) × 16000）。0 番目が 28928、1 番目が 28929、279999 番目が −18753（derived。元の i = 160000 から 439999 を切る。算術は 160000 mod 65536 = 28928、439999 mod 65536 = 46783 を符号つきにして −18753。2026-10-22 にシェルの算術で打った） | 陽性 | `two_speaker_clip_writes_the_samples_of_the_window` |
| T-RP-60-021 | 窓 mA の 25.000 から 30.000（元の音声の終わりに一致）。元の音声は S60 | 終わりが音声の終わりに一致（境界の内側） | 終わり ≤ 音声の長さ | `Ok` で、80000 sample。0 番目が 6784、79999 番目が 21247（derived。400000 mod 65536 と 479999 mod 65536。2026-10-22 にシェルの算術で打った） | 境界、陽性 | `two_speaker_clip_accepts_a_window_ending_at_the_end_of_the_audio` |
| T-RP-60-022 | 窓 mA の 25.000 から 30.001。元の音声は S60 | 終わりが音声の終わりを 1 ms（16 sample）越える | 終わり ≤ 音声の長さ | `Err` で、元の wav の path を名指し、窓が音声の終わりを越えることを言う。短い wav を黙って書かない（implicit。短い wav を書くと manifest の終わりが嘘になる。021 が同じ始まりの陽性対照） | 境界、陰性 | `two_speaker_clip_rejects_a_window_ending_past_the_audio` |
| T-RP-60-023 | 020 の切り出しが書いた manifest を読む | manifest の会議の名前、始まり、終わり | 3 項目 | 会議の名前が `mA`、始まりが 10.000、終わりが 27.500（ms の整数で 10000 と 27500）（specified、RP-60 の「manifest に会議の名前、始まり、終わり、sha256 が在る」の前の 3 項目） | 陽性 | `two_speaker_clip_manifest_names_the_meeting_and_the_window` |
| T-RP-60-024 | 020 の切り出しが書いた manifest の sha256 と、書いた wav の bytes | manifest の sha256 | 一致 | sha256 の項目が 64 字の小文字の 16 進で、書いた wav の file の bytes の SHA-256 と同じ（specified、RP-60 の sha256 の項目。pseudo：`sha.rs` の `sha256_hex` で file を引く。`sha.rs` は NIST の標準 vector `abc` の test で別に確かめてある。「sha256 は記録だけ」の読みで、元の wav の sha256 と比べる検査は持たない） | 陽性 | `two_speaker_clip_manifest_sha256_is_the_digest_of_the_written_wav` |
| T-RP-60-025 | 020 の切り出しを、S60 の 1 sample だけ値を変えた 4 通り（変える sample は 159999、160000、439999、440000）で打ち、それぞれの sha256 を、S60 を変えない 020 の切り出しの sha256 と比べる | 窓の始まりの 1 つ手前、始まり、終わりの 1 つ手前、終わり（終わりは含まない） | 窓の中だけが効く | 159999 と 440000 は sha256 が同じ。160000 と 439999 は違う（derived。窓は 160000 以上 440000 未満の sample。窓の外を巻き込む build と、窓の端を 1 sample 欠く build は落ちる） | 境界 | `two_speaker_clip_sha256_changes_with_a_sample_inside_the_window_only` |
| T-RP-60-026 | 020 の切り出しを、別の 2 つの dir に 2 回打つ | 同じ入力の 2 回 | byte 単位で同じ | 2 つの wav の bytes が同じで、2 つの manifest の bytes が同じ（derived、`manifest.rs` の「同じ入力を 2 回 pin すると同じ bytes を出す」と同じ性質。時刻や乱数を書かない） | 性質 | `two_speaker_clip_is_byte_identical_when_cut_twice` |
| T-RP-60-027 | 受入：実走。`testdata/eval_diar/` の 5 会議の RTTM と wav（git 管理外）を、会議の名前の順に読んで窓を選び、scratch の dir に wav と manifest を書く | 実物（AMI の 5 会議） | 規則に合う最初の窓 | 窓は IS1008a の 457.220 から 491.880。窓の 4 区間は MIO086 457.220 15.990、MIO086 473.530 9.960、FIE073 485.160 3.560、MIO086 489.320 2.560 で、話者は 2 人、少ない方の FIE073 は 1 区間で 3.560 秒、窓の中の重なりは 0、窓の外の区間が窓へ入る時間も 0 ms。ES2004a と ES2011a に、窓の中の重なりが 0 で、窓の外の区間が 1 ms も入らず、少ない方の区間が 2〜4 秒の窓は無い。窓の外の条件だけで落ちる窓が、ES2004a に 1 つ（215.090 から 230.100。少ない方の FEE013 は 2.530 秒で、外の区間が 1010 ms 入る）、ES2011a に 3 つ（38.260 から 109.810 で 430 ms、457.200 から 494.930 で 3150 ms、461.830 から 503.140 で 600 ms。少ない方は 3.000、3.320、3.320 秒）在る。wav は 554560 sample で、先頭の 4 sample が −542、−572、−621、−683、最後の 2 sample が 265、284。manifest は会議の名前 `IS1008a`、始まり、終わり、書いた wav の sha256 を持つ（pseudo：別の式（awk）で、会議の名前の順に窓が見つかる IS1008a までの 3 会議の RTTM を、ミリ秒の整数に読んで数えた。連続する 4 区間のうち、話者が 2 人で少ない方が 1 区間、窓の中の重なりが 0、窓の外の区間が 1 ms も入らない窓（長さの条件の前の絞り込み）の、少ない方の区間の長さは、ES2004a で 0.08〜0.33 秒（6 窓）、ES2011a で 1.74 と 23.36 秒、IS1008a で 6.33、1.41、0.24、9.02、3.56 秒。2〜4 秒に入るのは IS1008a の 3.56 秒だけ。窓の外の条件を外して数え直すと、少ない方が 2〜4 秒の窓として、上の ES2004a の 1 窓と ES2011a の 3 窓が加わる。sample は `od` で元の wav の 98 byte の先頭の後ろ（sample 7315520 から）を読んだ。2026-10-22 に打った。ASSUMPTION：窓の読みを設計の席が替えたら、この行の期待値を打ち直す）。この関数には `#[ignore = "reads testdata/eval_diar, which is not in git"]` を付ける（`testdata/` は `.gitignore` の 13 行目で git の外）。`--skip` は足さない。打つ時は command に `-- --ignored` を足す | 陽性 | `two_speaker_clip_is_cut_by_the_rule_from_ami` |
| T-RP-60-028 | 002 の B の長さを 1.900 にする | 下の境界の外側（0.1 秒外） | 2.000 秒以上 | `Err` で、規則に合う窓が無いことを名指す（specified、評議の決め：「2〜4 秒」は両端を含み、1.9 秒は外。002 が陽性対照） | 境界、陰性 | `two_speaker_clip_rejects_a_minority_turn_of_1_9_seconds` |
| T-RP-60-029 | 004 の B の長さを 4.100 にする | 上の境界の外側（0.1 秒外） | 4.000 秒以下 | `Err` で、規則に合う窓が無いことを名指す（specified、同上。004 が陽性対照） | 境界、陰性 | `two_speaker_clip_rejects_a_minority_turn_of_4_1_seconds` |
| T-RP-60-030 | mA：A 0.131 3.000、A 3.131 1.000、B 5.000 3.000、A 9.000 1.000（同じ話者の 2 区間が接する。f64 で 3.1310000000000002 と 3.1309999999999998） | 同じ話者どうしの接する境界（重なり 0 の内側） | 重なり 0 | 窓は mA の 0.131 から 10.000（specified、評議の決め：接するだけ（時間の積 0）は重なりでない。ms の整数で比べない build は落ちる） | 境界、陽性 | `two_speaker_clip_accepts_same_speaker_turns_that_touch` |
| T-RP-60-031 | 030 の 2 番目の区間を A 3.130 1.000 にする（同じ話者の 2 区間が 1 ms 重なる） | 同じ話者どうしの重なり 1 ms（外側の最小） | 重なり 0 | `Err` で、規則に合う窓が無いことを名指す（specified、評議の決め：同じ話者の区間どうしの重なりも重なりに数え、時間の積が 1 ms 以上なら重なり。030 が陽性対照） | 境界、陰性 | `two_speaker_clip_rejects_a_same_speaker_overlap_of_1_ms` |
| T-RP-60-032 | F60v に長さ 0 の区間 C 21.000 0.000 を足す（5 行） | 長さ 0 の区間は窓を選ぶ前に捨てる。足せば話者が 3 人、区間が 5 つになる | 捨てた後で判定 | 窓は mA の 10.000 から 27.500、`Err` でない（specified、評議の決め：長さ 0 以下の区間は 4 区間に数えず、`Err` にもしない。001 と同じ窓） | 境界、陽性 | `two_speaker_clip_drops_a_zero_length_turn_before_choosing` |
| T-RP-60-033 | A 10.000 5.000、B 16.000 3.000、A 20.000 4.000、A 25.000 0.000（長さのある区間は 3 つ） | 長さ 0 の区間を 4 つ目に数えない | 区間は 4 つ | `Err` で、規則に合う窓が無いことを名指す（specified、同上。数える build は 4 区間と読んで窓を返し、ここで落ちる。010 と同じ 3 区間の入力に 0 長の行を足した形） | 境界、陰性 | `two_speaker_clip_does_not_count_a_zero_length_turn_as_the_fourth` |
| T-RP-60-034 | F60v に A 9.000 2.000 を足す（5 行。足した区間は 9.000 から 11.000 で、窓の始まり 10.000 へ 1000 ms 入る） | 窓の外の区間が窓の始まりへ入る。窓の外の条件の外側（入る時間が 1 ms 以上） | 窓の外の区間が窓へ入る時間は 0 | `Err` で、規則に合う窓が無いことを名指す。`Ok` の窓を返さない（specified、`rust_port.ja.md` の 8 の小節「窓の外の区間が、窓の始まりから終わりまでと 1 ms 以上重なる窓は採らない」。pseudo：別の式（awk）で同じ 5 行の連続する 4 区間を数えると、候補は 2 つで、9.000 から 24.000 は窓の中の A どうしが重なり、10.000 から 27.500 は外の A が 1000 ms 入るので、採れる窓は 0。2026-10-22 に打った）。001 が同じ 4 行の陽性対照で、落ちる理由は足した 1 行だけ。窓の外の条件を持たない build は 10.000 から 27.500 を返して落ちる | 陰性 | `two_speaker_clip_rejects_a_window_an_outside_turn_reaches_into` |
| T-RP-60-035 | F60v に C 27.500 1.000 を足す（5 行。足した区間は 27.500 から 28.500 で、窓の終わり 27.500 に接するだけ） | 窓の外の区間が窓の終わりに接する。窓の外の条件の内側（入る時間が 0） | 窓の外の区間が窓へ入る時間は 0 | 窓は mA の 10.000 から 27.500 で、`Err` でない（specified、同上「接するだけ（重なる時間が 0）なら採る」。pseudo：別の式（awk）で同じ 5 行を数えると、採れる窓は 10.000 から 27.500 の 1 つ。C は 3 人目の話者だが窓の外なので、窓の話者は 2 人のまま。2026-10-22 に打った）。034 の対の陽性。接するだけを重なりと読む build（入る時間が 0 以上で落とす build）は `Err` を返して落ちる | 境界、陽性 | `two_speaker_clip_accepts_a_window_an_outside_turn_only_touches` |
| T-RP-60-036 | `run` と同じ 2 段（`Cli::try_parse_from` と `execute`）で `two-speakers --eval-diar <dir> --out <out>` を打つ。`<dir>` は mA.rttm（F60v）と mA.wav（S60）を置いた scratch の dir、`<out>` は別の scratch の dir（まだ無い）。陰性の側は同じ引数から `--out <out>` を除いた形 | 引数が 2 つ揃う形（陽性）と、必須の引数が 1 つ欠ける形（陰性）。どちらの引数も既定を持たない | `--eval-diar` と `--out` の 2 つが揃うこと | 揃う形：`Ok` で、`<out>` に `two_speakers.wav` と `two_speakers.json` が在る。wav は 16 kHz、16 bit、mono で 280000 sample、0 番目が 28928、279999 番目が −18753（derived、020 と同じ窓と同じ算術）。manifest の text が `CC BY 4.0` と `https://groups.inf.ed.ac.uk/ami/corpus/license.shtml` と会議の名前 `mA` を持つ（specified、`rust_port.ja.md` の「材料の作り方と置き場」の manifest の帰属。key の綴りは実装の席が決めるので、test は text に値の字が在ることを見る）。検索の陰性対照：同じ text に別の license の綴り `CC BY-NC` は無い。欠ける形：parse が `Err` で、`ErrorKind::MissingRequiredArgument`、`exit_code()` が 2（specified、同じ小節の「2 つの引数は既定を持たず、欠けたら引数の誤りで止まる」。clap の `MissingRequiredArgument` は、必須の引数が 1 つ以上与えられない時の種類）。揃う形が欠ける形の陽性対照で、違いは `--out` の有無だけ。帰属を書かない build は揃う形で落ち、`--out` に既定を持たせて黙って書く build は欠ける形で落ちる | 陽性、陰性 | `two_speaker_clip_command_writes_the_files_the_diarization_tests_read` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

1. 窓の長さと、区間の間の無音には上限を置かない（設計の席の決めで、`rust_port.ja.md` の 8 の R12u1 に在る）。窓の外の区間が窓へ 1 ms 以上入る窓は採らず、接するだけは採る（034、035）。この節の行は、上の「窓の読み」（始まりで並べた連続する 4 区間。複数あれば会議の名の辞書順、始まりの昇順で最初の 1 つ）のままで置いた。区間が 5 つの列の行は置いていない。読みが替わったら 010、027、034、035 の期待値を打ち直す。
2. 「2〜4 秒」の両端は含む。比べるのは整数の ms で、1.9、2.0、4.0、4.1 秒の標本が境界である（002 から 005、028、029）。
3. 同じ話者の区間どうしの重なりも重なりに数え、時間の積が 1 ms 以上なら重なり、接するだけ（積 0）は重なりでない（030、031）。
4. 長さが 0 以下の区間は、窓を選ぶ前に捨てる。4 区間に数えず、`Err` にもしない（032、033）。
5. sha256 は、書き出した wav の file の bytes を取る（024、025）。
6. 027 の関数は `#[ignore = "reads testdata/eval_diar, which is not in git"]` を付ける。`--skip` は足さない。
7. 会議の名前の順は、文字列の昇順とした。AMI の名前は同じ長さで、数字の桁の違いは実物に無い。

### 4.84 RP-61（R13-19b の固定の 4 本）

test：`native/hayamimi-eval/tests/rp61.rs`。受入 test の名前は `the_comma_and_the_question_mark_each_pass_in_a_sentence_written_out`、`a_sample_that_waited_119_ms_is_not_valid_and_one_of_120_ms_is`、`the_p95_is_the_nearest_rank_value_and_a_rank_off_by_one_is_not`、`a_directory_missing_one_runs_samples_names_that_file_alone`。
この節の行は R13-19 の `rp61.rs` の fixture の test を強める 4 本（001 から 004）と、その検出器の対照 2 本（005、006）である。窓 E4 の記録を読む実走の行は持たない。全行が `--ignored` を付けずに通る。
command：`cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp61`。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。値は定数（`MARKS`、`VALID_MS`）を回して作らず、行の数を直書きする。
設計は D 8 の R13-19b と D 6 の RP-61 である。

**数え方**：

- 有効な標本は、待ち `d` が 120 ms 以上の標本である（`VALID_MS` = 120）。`d` は `(t_final_us − t_end_us) / 1000 − final_latency_ms`。
- p95 は nearest-rank で、n 個の p% は小さい方から `(p × n + 99) / 100`（整数の割り算）番目である（T 4.1）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-61-001 | 受入：ja の清書の run を 2 つ作る。1 つは `refine` の event が 1 件で `lang` が `ja`、`text` が `予算は五百万円です、`。もう 1 つは同じ形で `text` が `本当ですか？`。どちらの文も他の 2 つの記号を持たない（直書き） | 記号 3 つのうち `、` だけの文と `？` だけの文 | 記号を持つ ja の `refine` が 1 件以上 | 2 つとも判定が通り、`1 of 1 ja refine events hold a mark` が出る（specified、RP-61 の「清書で句読点が付く」。`MARKS` を回した文でなく直書きの文で見る）。陰性：`MARKS` から `？` を外した build は 2 つ目で、`、` を外した build は 1 つ目で `0 of 1 ja refine events hold a mark` を出して落ちる | 陽性、陰性 | `the_comma_and_the_question_mark_each_pass_in_a_sentence_written_out` |
| T-RP-61-002 | 受入：1 つの run に標本 3 つ。`d` が 119 ms、120 ms、121 ms。3 つとも訳が取れている | `VALID_MS` の境界。1 ms 下、ちょうど、1 ms 上 | `d` ≥ 120 ms | 判定が読む有効な標本の数が 2（120 ms と 121 ms）。119 ms は有効から外れ、120 ms は入る（specified、3.19 の有効の線。前例は `rp67.rs` の 120 と 300 を比べる test）。陰性：`VALID_MS` を 121 にした build は 1、119 にした build は 3 になって落ちる | 境界、陽性、陰性 | `a_sample_that_waited_119_ms_is_not_valid_and_one_of_120_ms_is` |
| T-RP-61-003 | 受入：refine の run の標本 20 個。すべて有効で訳が取れ、`L` が 10、20、…、200 ms（10 ms 刻みで全部異なる）。別に 21 個の run を作る：`L` が 10、20、…、210 ms | 順位 ⌈0.95 n⌉ の値と、その前後 1 位の値が標本に在る n | nearest-rank | n = 20 の p95 は 19 番目の 190 ms（前後は 180 ms と 200 ms）。n = 21 の p95 は `(95 × 21 + 99) / 100` = 20 番目の 200 ms（前後は 190 ms と 210 ms）（specified、T 4.1 の順位の式。pseudo：`awk` で `int((95*n+99)/100)` を打った）。陰性：順位を 1 下げた build は 180 ms と 190 ms、1 上げた build は 200 ms と 210 ms を返して落ちる。切り捨ての `0.95 × 21` = 19 番目（190 ms）を採る build も n = 21 で落ちる | 境界、陰性 | `the_p95_is_the_nearest_rank_value_and_a_rank_off_by_one_is_not` |
| T-RP-61-004 | 受入：test の中で `env!("CARGO_TARGET_TMPDIR")` の下に dir を作り、6 つの標本の file（`refine.e2e1.jsonl`、`refine.e2e2.jsonl`、`norefine.e2e1.jsonl`、`norefine.e2e2.jsonl`、`python.e2e1.jsonl`、`python.e2e2.jsonl`）のうち `python.e2e1.jsonl` だけを置かない | 6 つのうち 1 つだけ欠ける | 欠けた file だけを名指す | `load_runs` の `Err` が `1 of the 6 runs` と `python.e2e1.jsonl` を持ち、在る 5 つの file の名前を持たない（specified、RP-61 の「記録の欠けは名指す」。`python.e2e1.jsonl` は在る 5 つのどの名前の部分にもならない名前を選んだ）。陰性：欠けた file の名前を出さない build と、6 つ全部の名前を並べる build は落ちる | 境界、陰性 | `a_directory_missing_one_runs_samples_names_that_file_alone` |
| T-RP-61-005 | 001 の 1 つ目の run の `text` を `予算は五百万円です`（3 つの記号のどれも持たない）にする | 検出器の陰性対照 | 記号が 0 件で落ちる | 判定が落ち、`0 of 1 ja refine events hold a mark` を出す（specified、同上。001 が同じ形の陽性対照で、記号が数えられていることを示す） | 陰性 | `a_sentence_with_none_of_the_three_marks_is_rejected` |
| T-RP-61-006 | 002 の標本を、`d` が 119 ms の 1 つだけにする | 有効が 0 | 有効な標本が在る | 有効な標本の数が 0 で、判定は 0 件を通さず落ちる（implicit。0 件の p95 を作らない。002 の 2 が陽性対照） | 境界、陰性 | `a_run_whose_samples_are_all_under_120_ms_has_no_valid_sample` |

### 4.85 RP-89〜RP-95（長い buffer の復号の上限）

test：`native/hayamimi-asr/tests/rp89.rs`、`native/hayamimi-asr/tests/rp90.rs`、`native/hayamimi-asr/tests/rp91.rs`、`native/hayamimi-asr/src/cap.rs` の `mod tests`、`native/hayamimi-eval/tests/rp93.rs`、`native/hayamimi-asr/tests/rp94.rs`、`native/hayamimi-eval/tests/rp95.rs`。7 つとも作る物で、書く席は LB-0 が rp89.rs、LB-1 が rp90.rs と rp91.rs と cap.rs、LB-3 が rp93.rs と rp94.rs と rp95.rs である。case の `T-RP-<n>-` の `<n>` が 89 なら rp89.rs、90 なら rp90.rs、91 なら rp91.rs、92 なら cap.rs、93 なら rp93.rs、94 なら rp94.rs、95 なら rp95.rs に置く。受入 test の名前は `rp89_the_whole_decode_drops_characters_on_a_long_buffer`、`rp90_a_capped_decode_keeps_the_characters_of_a_long_buffer`、`rp91_a_buffer_within_the_cap_reads_the_same_as_before`、`a_long_buffer_is_cut_at_its_quietest_frames_into_balanced_pieces`、`rp93_the_default_cap_is_the_one_the_sweep_chose`、`rp94_a_capped_decode_adds_at_most_150_ms_p50_and_250_ms_p95`、`rp95_the_cap_does_not_delay_the_subtitle`。
入力が「受入：実走」か「実走：」で始まる行は model と `testdata/long_ja/` を読み、`#[ignore = "reads testdata/long_ja and the models, which are not in git"]` を付ける。RP-94 と RP-95 の実走は窓 E4 だけで走るので `#[ignore = "runs only in window E4"]` を付ける。ほかの行は model も `testdata/` も読まず、`--ignored` なしで通る。`--skip` は足さない。
command：RP-92 は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-asr --lib cap`。RP-93 は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-eval --test rp93`。RP-89、RP-90、RP-91、RP-94 は `cd native && flock -w 1800 target/release/.cargo-lock true && sudo -n nice -n -5 cargo test --release --jobs 2 -p hayamimi-asr --test rp<n> -- --ignored`（`<n>` は 89、90、91、94）。RP-95 は同じ形で `-p hayamimi-eval --test rp95`。fixture の行だけを打つ時は `-- --ignored` を外す。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。受入の行の関数の末尾は、D 7 の受入 test の名前そのものである。
対照の列は、D 7 の「陰性の case」に載る名前の行を陰性、受入が通る側の行と D 6 が陽性対照と呼ぶ行を陽性にする。D 7 の陰性の case の名前 9 つと受入 test の名前 7 つは、どれも関数の末尾にそのまま置いた。
設計は D 3.22 と、D 6 の RP-89 から RP-95、D 7 の名前の表、D 10 の H150 から H164 である。

**長い材料**（LB-0 の席が `testdata/long_ja/` に作る。`testdata/` は git の外）：

- 元は `testdata/eval_real/ja_*.wav`（16 kHz、mono、16 bit）である。標本の数は 2026-10-22 に、15 本それぞれへ `ffprobe -v error -select_streams a:0 -show_entries stream=sample_rate,channels,duration_ts -of csv=p=0` を打って取った（pseudo。wav の長さの単位が標本）。
- 組は重ならない 3 本で、3 つ。clip の順は次のとおりで、順を変えない。
  - **L1**：`ja_10`（140721 標本）、`ja_13`（95080）、`ja_15`（81017）。声の合計は 316818 標本、19.801125 秒。参照の字数（NFKC にして句読点と空白を除く。`cer.rs:45` の `normalize_ja` と同じ）は 24 + 13 + 19 = 56。
  - **L2**：`ja_05`（134268）、`ja_08`（104263）、`ja_07`（76185）。声の合計は 314716 標本、19.669750 秒。参照の字数は 23 + 30 + 12 = 65。
  - **L3**：`ja_03`（125012）、`ja_09`（110867）、`ja_04`（87240）。声の合計は 323119 標本、20.194938 秒。参照の字数は 32 + 17 + 13 = 62。
- 間は、16 kHz の値 0 の標本を clip と clip の間にだけ置く（最初の clip の前にも、最後の clip の後ろにも置かない）。0.2 秒は 3200 標本、0.5 秒は 8000 標本。繋いだ長さ（標本、秒）は、L1 が 0.2 秒で 323218（20.201125）、0.5 秒で 332818（20.801125）、L2 が 321116（20.069750）と 330716（20.669750）、L3 が 329519（20.594938）と 339119（21.194938）。6 本とも 20 秒以上 25 秒以下に入る。最短は L2 の 0.2 秒で、20 秒との差は 1116 標本（0.06975 秒）である。標本の数は shell の算術で足した（pseudo）。
- 参照は 3 本の `ref`（`testdata/eval_real/manifest.json`）を、この順に、区切りなしで繋いだ文である。L1 は `勝利はしたものの１０万人以上の戦死者を出していた。` `高校生２００人に聞きました。` `模擬裁判に参加した大学１年生の本音とは。` を、L2 は `でも敦賀さん、９１歳力強い走りに変わっていきます。` `けさの東京、風はなさそうですけどね、体感はどうでしょうか、近藤さん。` `しっかり打ち切れるかです。` を、L3 は `そこの上流から丸太を組んでいかだにしてどんどん流したんですこれを。` `そしてもう一つ大きな勢力があります。` `今、１００ｍのターンを迎える。` を、それぞれ繋ぐ。字数は `jq` で `ref` を引き、`perl -CSD -MUnicode::Normalize` で NFKC にして句読点と空白を除いて数えた（pseudo。15 本の字数は ja_01 から ja_15 の順に 15、10、32、13、23、26、12、30、17、24、7、18、13、32、19）。
- `ja_06` を使わない。`docs/results/scorecard.md:37` が、`ja_06` は音声に在って参照文に無い冒頭の発話まで model が出す（参照の不備）と書いており、組に入れると 1 本ずつの率が見かけ上悪くなって差の読みが濁る。D 3.22 の組の例は、間 0.2 秒で 2 組が 20 秒に届かず（19.716 秒と 19.788 秒）、1 組が `ja_06` を含むので採らなかった。
- 作り方：`hayamimi-eval fetch-set --concat` で足りる（`cli.rs:182-202` の引数と `manifest.rs:389-449` の `concat` を読んだ。built binary が無いので `--help` は打っていない。NOT VERIFIED）。組ごとに、`ja_NN.wav` を上の順に並べた 3 行の manifest を `testdata/long_ja/` に置き（`wav` は `../eval_real/ja_NN.wav`）、`--pin` で sha256 を足し、`--part manifest:<pinned>:ja --gap-ms 200` と `--gap-ms 500` で繋ぐ。`concat` は間を clip の間にだけ入れ、最後の後ろに入れない（`manifest.rs:385-388` の doc）。**`--gap-ms` の既定は 1500 である**（`cli.rs:196-197`）ので、付け忘れると 1.5 秒の間の材料ができる。T-RP-89-008 が実物で、T-RP-89-010 が判定の関数で、これを止める。

**数え方**：

- 誤りと字数は `native/hayamimi-eval/src/cer.rs` の `cer`（`score.rs:52` の ja の腕と同じ）で数える。複数の buffer の率は、`Cer::plus`（`cer.rs:26`）のとおり dist と denom をそれぞれ足してから割る（率の平均は取らない）。micros は `dist × 1_000_000 / denom` の整数の割り算で、1.0 pt は 10_000 micros。
- **全体**は、組の buffer を 1 回で復号した文（上限なし。`decode.rs:377` の `decode_whole` と同じ形）。**1 本ずつ**は、組の 3 本を元の clip のまま（間なし）、同じ復号で 1 本ずつ復号して参照と照らし、3 本の dist と denom を足した物。1 本ずつの率は間の長さによらないので、6 本（3 組 × 間 2 通り）の合算では、1 本ずつの dist と denom を 2 回ずつ数える（合算の denom は 56 + 65 + 62 の 2 倍で 366）。
- 「字の数の比」は、`normalize_ja` を通した仮説の字数 ÷ 参照の字数。判定は整数で、`仮説 × 10 ≥ 参照 × 8` で 0.8 以上を見る。
- 全体と 1 本ずつの両方に、同じ後処理（数字の正規化の有無）を掛ける。差で比べるので、数字の効きは打ち消し合う（ASSUMPTION。D 6 の RP-89 は後処理を言っていない）。

**RP-92 の fixture**（標本率 16000。T と C と W は標本の数で書く）：

- 枠は 320 標本（20 ms）で、枠 i は標本 320·i 以上、320·(i + 1) 未満。`C` = 160000（10 秒）、`W` = 32000（2.0 秒。`W/2` = 16000 標本 = 50 枠）、`O` = 0。
- 音の素は 3 つ。**tone**は周期 16 標本の矩形波（8 標本が +0.5、8 標本が −0.5）で、どの枠も同じ並びなので RMS は 0.5 で、枠どうしが bit で等しい。**dip**は同じ形で振幅 0.25（RMS 0.25）。**zero**は全部 0.0（RMS 0）。「枠 i を zero にする」は、tone の buffer の枠 i の 320 標本を 0.0 に置き換えること。
- **F24**：T = 384000（24 秒）、N = 3、的は 128000 と 256000。窓の中の枠は 350〜449 と 750〜849。**F16**：T = 256000（16 秒）、N = 2、的は 128000。窓の中の枠は 350〜449。窓は半開区間で、F16 は [112000, 144000)。先頭がこの区間に入る枠は 350 以上 450 未満（枠 450 の先頭は 144000 = `hi` で外、枠 350 の先頭は 112000 = `lo` で内）。
- 枠の格子の起点は buffer の先頭、切れ目は選んだ枠の先頭の標本、窓と同値の選び方は D 3.22 の「切り方の決まり」1〜5 による。切れ目の期待は標本の位置そのもので書く。決まり 1〜5 を素朴に全探索で書いた awk を、tone だけの buffer の T = 400000、160001、160320、320000、320001、320320、319680 で打ち、011 から 016 と 022 の切れ目を得た（pseudo。2026-10-22）。

**RP-93 の fixture**（`sweep.tsv` の形は D 3.22 の「`sweep.tsv` の形」。tab 区切り、UTF-8、1 行目が見出しの 14 列）：

- 列は順に `cap_s`、`overlap_s`、`live_rp90`、`live_rp91`、`refine_rp90`、`refine_rp91`、`scorecard_rp90`、`scorecard_rp91`、`rp94`、`rp94_p50_ms`、`rp94_p95_ms`、`long_err_micros`、`retry_fired`、`chosen`。経路は live、refine、scorecard の 3 つ。通否の列は `pass`、`fail`、`-`（測っていない。通っていない側に数える）。行は `cap_s` ∈ {4, 6, 8, 10, 14} × `overlap_s` ∈ {0, 0.5} の 10 行と、`off` の 1 行（`overlap_s` は `-`、`chosen` は常に 0）。
- **資格のある行**は、6 つの経路の列が全部 `pass` で、`rp94` が `pass` の行。`off` の行は候補に数えない。`chosen` は選んだ行だけ 1。
- **F93a**：`C` ≤ 10 の 8 行が全部通る。`C` = 14 の 2 行は RP-94 だけが落ちる。資格のある行の `C` の最大は 10 で、`O` = 0 と 0.5 が同点なので選ぶ行は (10, 0)。**F93b**：F93a で (10, 0) の live の RP-90 を落とす。選ぶ行は (10, 0.5)。**F93c**：F93a で `C` = 10 の 2 行の refine の RP-91 を落とす。選ぶ行は (8, 0)。**F93d**：F93a で `C` = 10 の 2 行の RP-94 を落とす。選ぶ行は (8, 0)。**F93e**：全部の行の live の RP-90 が落ちる。資格のある行は 0。
- dump の上限の行は、`off` か、`C` と `O` の組。key の綴りは LB-1 の席が決める。dump が `off` でも選んだ行でも通す（D 3.22 の `sweep.tsv` の形の末尾）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-89-001 | 受入：実走。長い材料 6 本（L1、L2、L3 の 0.2 秒と 0.5 秒）。LB-0 の席が今の build で打つ。各 buffer を上限なしの Rz で 1 回で復号した文（全体）と、組の 3 本を 1 本ずつ復号した文を、組の参照と照らす | 20.07〜21.19 秒の buffer（`C` = 10 秒の 2.0〜2.1 倍）。落ちる側の同値類 | 全体の合算の率 − 1 本ずつの合算の率 ≥ 5.0 pt（50_000 micros。D 6 の仮） | 差が 50_000 micros 以上（specified、RP-89。整数の micros で比べる）。run は 6 本それぞれの全体の率、1 本ずつの率、全体の字数 ÷ 参照の字数を出力に並べる。差が出なければ RP-89 は落ち、LB-1 へ進まず評議の席へ出す（D 6 の陰性の列） | 陽性 | `rp89_the_whole_decode_drops_characters_on_a_long_buffer` |
| T-RP-89-002 | 実走：既に在る陽性の対照の形。`ja_01.wav` から `ja_08.wav` を、各 clip の後ろに 8000 標本（0.5 秒）の無音を置いて繋いだ 827938 標本（51.746125 秒）。`decode.rs:277` の `long_buffer` と同じ buffer | 50 秒を超える側の同値類 | 差 ≥ 5.0 pt | 差が 50_000 micros 以上（derived：`decode.rs:301` が全体の率を分け直しの率より 100_000 micros を超えて悪いと確かめ、`:304` が字の数を半分未満と確かめる。分け直しの率が 1 本ずつの率に近いことは ASSUMPTION）。827938 = 763938 + 8 × 8000（pseudo：`ffprobe` の標本の和を shell の算術で打った）。`ja_06` を含むが、`ja_06` の参照の不備は全体にも 1 本ずつにも同じ量で入るので、差では打ち消し合う | 陽性 | `the_existing_51_second_buffer_shows_the_same_drop` |
| T-RP-89-003 | 実走：1 本だけの組。9 本の clip（L1、L2、L3 の元の clip）を、間なしの 1 本の buffer として、全体の復号と 1 本ずつの復号の両方にかける | 長さが `C` 以下（4.76 秒以上 8.80 秒以下）。buffer が clip と同一 | 差 < 1.0 pt（10_000 micros 未満） | 9 本とも差が 10_000 micros 未満（specified、D 6 の RP-89 の陰性の列。derived：入力が同じなら同じ文が出るので差は 0 のはずで、0 でなければこの行が測りの床を出す）。001 が同じ復号を長い buffer に当てた陽性 | 陰性 | `a_single_clip_group_shows_no_drop` |
| T-RP-89-004 | 判定の関数（micros の対を受ける）に、全体 150_000 micros、1 本ずつ 100_000 micros | 5.0 pt の境界の内側（ちょうど） | 差 ≥ 50_000 micros | `Ok`（specified、RP-89。差 50_000 micros ちょうどは通る） | 境界、陽性 | `a_drop_of_exactly_5_points_is_enough` |
| T-RP-89-005 | 判定の関数に、全体 149_999 micros、1 本ずつ 100_000 micros | 5.0 pt の境界の外側（1 micro 外） | 差 ≥ 50_000 micros | `Err` で、差が 49_999 micros で足りないことを言う（specified、同上。004 が同じ 1 本ずつの値の陽性対照で、違いは全体の 1 micro だけ） | 境界、陰性 | `a_drop_of_4_9999_points_is_not_enough` |
| T-RP-89-006 | 1 本だけの組の判定の関数に、全体 109_999 micros、1 本ずつ 100_000 micros | 1.0 pt の境界の内側（9_999 micros の差） | 差 < 10_000 micros | `Ok`（specified、RP-89 の陰性の列「1.0 pt 未満」） | 境界、陰性 | `a_difference_of_9999_millionths_is_no_drop` |
| T-RP-89-007 | 1 本だけの組の判定の関数に、全体 110_000 micros、1 本ずつ 100_000 micros | 1.0 pt の境界の外側（ちょうど。「未満」なので外） | 差 < 10_000 micros | `Err` で、差が 10_000 micros で落ちが見えていることを言う（specified、同上。006 が 1 micro 内側の対） | 境界、陰性 | `a_difference_of_10000_millionths_is_a_drop` |
| T-RP-89-008 | 実走：`testdata/long_ja/` の 6 本の wav を `read_wav` で読み、元の 9 本の clip と並べる | 材料の形。標本の数と、間の標本と、clip の標本 | 6 つの標本数、間の 0、clip の一致 | 標本数が 323218、332818、321116、330716、329519、339119。間の 3200 標本または 8000 標本が全部 0。clip の標本が元の wav の標本と 1 つ残らず一致（derived：上の標本の数と shell の算術。pseudo：別の式で、元の wav を同じ順に並べて比べる）。`--gap-ms` を付け忘れた 1500 ms の材料は、2 つの間が 48000 標本になり、期待より 0.5 秒の材料で 32000 標本、0.2 秒の材料で 41600 標本多くなって落ちる | 陽性 | `the_long_material_is_the_three_clips_joined_by_zero_gaps` |
| T-RP-89-009 | 材料を調べる関数に、3 つの部分 `[1, 2, 3]`、`[4, 5]`、`[6, 7, 8, 9]`（i16）と間 2 標本、繋いだ `[1, 2, 3, 0, 0, 4, 5, 0, 0, 6, 7, 8, 9]` | 材料の形が合う（陽性対照） | 間 2、前後に無音なし | `Ok`（derived：部分と間から手で並べた。010 から 013 が同じ入力を 1 か所ずつ壊した陰性） | 陽性 | `a_joined_buffer_with_exact_gaps_is_accepted` |
| T-RP-89-010 | 009 の最初の間を 1 標本にする `[1, 2, 3, 0, 4, 5, 0, 0, 6, 7, 8, 9]` | 間が 1 標本足りない（境界の外側） | 間 = 指定の標本数 | `Err` で、1 つ目の間の長さが 1 で 2 でないことを言う（implicit。009 が対の陽性） | 境界、陰性 | `a_gap_one_sample_short_is_rejected` |
| T-RP-89-011 | 009 の 2 つ目の間を `[0, 9]` にする | 間に 0 でない標本が 1 つ在る | 間の標本は全部 0 | `Err` で、2 つ目の間の 2 番目の標本が 0 でないことを言う（implicit。009 が対の陽性） | 陰性 | `a_nonzero_sample_in_a_gap_is_rejected` |
| T-RP-89-012 | 009 の後ろに 0 を 2 標本足す。別に、009 の前に 0 を 2 標本足す | 最後の clip の後ろ、最初の clip の前の無音 | 無音は clip の間にだけ | どちらも `Err` で、前または後ろに無音が在ることを言う（specified、D 3.22 の「材料」と `manifest.rs:385-388` の「最後の後ろに無音を置かない」。009 が対の陽性） | 境界、陰性 | `a_silence_before_the_first_or_after_the_last_clip_is_rejected` |
| T-RP-89-013 | 009 の `[4, 5]` を `[4, 6]` にする | clip の中の標本が元と 1 つ違う | clip は元と一致 | `Err` で、2 つ目の clip の 2 番目の標本が違うことを言う（implicit。009 が対の陽性） | 境界、陰性 | `a_clip_sample_that_differs_from_the_source_is_rejected` |
| T-RP-89-014 | 材料を読む関数に、存在しない scratch の dir（`testdata/long_ja/` ではない）を渡す | 材料が無い | 材料が在る | `Err` で、渡した dir の path を名指す。飛ばして通さない（specified、D 3.22 の「材料が無ければ、test は path を名指して落ちる」） | 境界、陰性 | `a_missing_long_material_names_its_path` |
| T-RP-89-015 | L1、L2、L3 の `ref` を `cer` に渡す。仮説は参照と同じ文、空文字列 | 参照の組み立てと字数 | 区切りなし、順どおり、denom が 56、65、62 | 参照の文が上の 3 つの文を区切りなしで繋いだ物と一致する。参照と同じ仮説で dist 0、空の仮説で dist が denom（56、65、62）（derived：`ref` の字数。pseudo：`jq` と `perl` で数えた。仮説が空でも 0 件を一致と読まない） | 陽性、陰性 | `the_reference_of_a_group_is_its_three_references_joined_in_order` |
| T-RP-90-001 | 受入：実走。長い材料 6 本を、上限 `C` = 10 秒、`O` = 0 を明示した Rz で読む（LB-1 の席が打つ）。1 本ずつの復号と、上限なしの全体の復号も同じ run で出す | 上限が 3 片に分ける長さ（N = 3） | 合算の率 ≤ 1 本ずつの合算の率 + 2.0 pt（20_000 micros）、かつ上限なしの合算の率より小さい。6 本それぞれで、字数の比 ≥ 0.8 | 3 つの判定が全部通る（specified、RP-90）。run は 6 本それぞれの率と字数の比を出力に並べる。`C` と `O` は引数で渡し、既定に頼らない | 陽性 | `rp90_a_capped_decode_keeps_the_characters_of_a_long_buffer` |
| T-RP-90-002 | 実走：001 と同じ 6 本を、上限 `off` を明示した Rz で読み、001 と同じ判定の関数に渡す | 上限なしの側（陽性の対照） | 001 と同じ | 判定が `Err`（specified、D 6 の RP-90 の陽性の列「上限なしで同じ判定が落ちる」。derived：RP-89 が通っていれば、全体の率は 1 本ずつの率より 50_000 micros 以上悪く、20_000 micros の線を超える）。001 が同じ buffer で通る側の対 | 陽性 | `the_uncapped_decode_fails_the_same_judgement` |
| T-RP-90-003 | 判定の関数に、上限ありの率 120_000 micros、1 本ずつの率 100_000 micros | 2.0 pt の境界の内側（ちょうど） | 差 ≤ 20_000 micros | 差の判定が通る（specified、RP-90 の「+ 2.0 pt 以下」。ほかの 2 つの判定は通る値を置く） | 境界、陽性 | `a_capped_error_2_points_above_the_clips_is_accepted` |
| T-RP-90-004 | 判定の関数に、上限ありの率 120_001 micros、1 本ずつの率 100_000 micros | 2.0 pt の境界の外側（1 micro 外） | 差 ≤ 20_000 micros | `Err` で、差が 20_001 micros で線を超えることを言う（specified、同上。003 が同じ 1 本ずつの値の対） | 境界、陰性 | `a_capped_error_one_millionth_above_the_line_is_rejected` |
| T-RP-90-005 | 判定の関数に、上限ありの率 120_000 micros、上限なしの率 120_000 micros、1 本ずつの率 100_000 micros。別に、上限なしの率 120_001 micros | 「上限なしより小さい」の境界。等しい側と、1 micro 小さい側 | 厳密に小さい | 等しい時は `Err` で、上限なしと等しいことを言う。上限なしが 120_001 micros の時は、この判定が通る（specified、RP-90 の「上限なしより小さい」。等しいを通す build は落ちる） | 境界、陰性 | `a_capped_error_equal_to_the_uncapped_error_is_rejected` |
| T-RP-90-006 | 字数の比の判定の関数に、参照 56 字（L1）、仮説 45 字 | 0.8 の境界の内側。56 × 0.8 = 44.8 なので 45 が最小 | 仮説 × 10 ≥ 参照 × 8 | `Ok`（450 ≥ 448。specified、RP-90 の「0.8 以上」。derived：56 × 8 = 448） | 境界、陽性 | `a_group_that_keeps_45_of_56_characters_passes` |
| T-RP-90-007 | 字数の比の判定の関数に、6 本のうち L1 の 0.2 秒だけを仮説 44 字（参照 56 字）、ほかの 5 本は 0.8 以上 | 0.8 の境界の外側（1 字外）。1 本だけが外れる | 6 本全部が 0.8 以上 | `Err` で、L1 の 0.2 秒の buffer を名指し、44 ÷ 56 を言う（specified、同上。440 < 448。「どの組も」を平均で読む build は落ちる）。006 が同じ参照の陽性 | 境界、陰性 | `one_group_at_44_of_56_characters_is_named` |
| T-RP-90-008 | 字数の比の判定の関数に、L3 の参照 62 字から `ja_09` の文（17 字）を除いた仮説（45 字） | 3 片の繋ぎで真ん中の 1 片を捨てた形（試験の中だけの繋ぎ） | 仮説 × 10 ≥ 参照 × 8 | `Err` で、L3 を名指し、45 ÷ 62 を言う（derived：450 < 496。D 7 の陰性の case「片の繋ぎで 1 片を捨てる build」）。捨てずに繋いだ 62 字は通る | 陰性 | `a_join_that_drops_a_piece_is_caught` |
| T-RP-90-009 | 実走：L3 の 0.5 秒の buffer を `Recognizer::decode` と `Recognizer::decode_tagged` で読む（`C` = 10 秒を明示） | 2 つの口が同じ上限を通る（D 3.22 の結論 2） | 文が byte で一致。札が `NotApplicable` | 2 つの文が byte で一致し、`decode_tagged` の札が `NotApplicable`（specified、D 3.22 の方針 2「片に分けた時は文を繋いだ物と `NotApplicable` を返す」）。2 つの口のどちらかが上限を通らない build は、文が 1 回の復号の文になって一致しない | 陽性 | `decode_and_decode_tagged_return_the_same_joined_text` |
| T-RP-90-010 | 片の文を繋ぐ関数に、`["こんにちは", "世界"]` | 日本語の字の境 | 間を空けない | `こんにちは世界`（specified、D 3.22 の切り方の決まり 6「日本語の字の間には空白を入れない」） | 陽性 | `japanese_sentences_are_joined_without_a_space` |
| T-RP-90-011 | 同じ関数に、`["hello", "world"]` | 両側が ASCII の英数字 | 半角空白を 1 つ | `hello world`（specified、決まり 6「両方が ASCII の英数字の時だけ、間に半角の空白を 1 つ」。空白を入れない build は 2 語が 1 語になって落ちる。010 が対） | 陽性 | `english_words_at_a_piece_boundary_get_one_space` |
| T-RP-90-012 | 同じ関数に、（a）`["hello", "世界"]`、（b）`["こんにちは", "world"]` | 片側だけが ASCII の英数字（2 つの向き） | 間を空けない | （a）は `hello世界`、（b）は `こんにちはworld`（specified、決まり 6「両方が」。どちらか一方で空白を入れる build は落ちる。011 が両側 ASCII の対） | 境界、陰性 | `a_space_needs_ascii_alphanumerics_on_both_sides` |
| T-RP-90-013 | 同じ関数に、`["  hello ", "", "world  "]`。別に、`["a", "", "b"]` | 前後の空白と、空の文 | 空白を除き、空の文を捨ててから境を見る | `hello world` と `a b`（specified、決まり 6「それぞれ前後の空白を除き、空の文を捨て、順に繋ぐ」。空の文を捨てる前に境を見る build は `a  b` か `ab` を返して落ちる） | 境界、陽性 | `blank_and_empty_sentences_are_dropped_before_joining` |
| T-RP-90-014 | 同じ関数に、（a）`["hello.", "world"]`、（b）`["abc", "123"]`、（c）`["10", "人"]` | ASCII の記号で終わる文、数字どうし、数字と日本語 | 英数字だけを見る | （a）は `hello.world`（`.` は英数字でない）、（b）は `abc 123`（数字は英数字）、（c）は `10人`（specified、決まり 6。derived：記号は英数字に数えない読み） | 境界、陽性 | `only_ascii_letters_and_digits_count_at_a_piece_boundary` |
| T-RP-90-015 | 重なりを畳む関数（`O` > 0）に、`("今日は晴れ", "晴れです")` | 2 字の一致（畳む最短） | 2 字以上 6 字以下の一致を 1 回だけ残す | `今日は晴れです`（specified、決まり 7。畳まない build は `今日は晴れ晴れです` を返して落ちる） | 境界、陽性 | `a_two_character_overlap_is_folded` |
| T-RP-90-016 | 同じ関数に、`("今日は", "はい")` | 1 字の一致（畳まない側の最大） | 1 字は畳まない | `今日ははい`（specified、決まり 7「1 字の一致は畳まない」。015 が対の陽性） | 境界、陰性 | `a_one_character_overlap_is_not_folded` |
| T-RP-90-017 | 同じ関数に、（a）`("んあいうえおか", "あいうえおかす")`、（b）`("んあいうえおかき", "あいうえおかきす")` | 6 字の一致（畳む最大）と 7 字の一致（範囲の外） | 一致は 2〜6 字 | （a）は `んあいうえおかす`。（b）は 2〜6 字で一致する長さが無いので畳まず `んあいうえおかきあいうえおかきす`（specified、決まり 7「2 字以上 6 字以下」。derived：（b）の末尾 6 字 `いうえおかき` と頭 6 字 `あいうえおか` は違い、2〜5 字も違う） | 境界、陽性、陰性 | `an_overlap_of_six_characters_is_folded_and_one_of_seven_is_not` |
| T-RP-90-018 | 同じ関数に、`("xxabab", "ababyy")` | 一致が 2 字と 4 字の両方で成り立つ | 最も長い一致を畳む | `xxababyy`（4 字を畳む。2 字だけ畳む build は `xxabababyy` を返して落ちる。specified、決まり 7「最も長い物」） | 境界、陽性 | `the_longest_overlap_is_the_one_folded` |
| T-RP-91-001 | 受入：実走。README の 15 本（`testdata/eval_real/ja_*.wav`）を、上限 `C` = 10 秒を明示した recognizer と `off` の recognizer で読む | 全部 `C` 以下（最長 140721 標本 = 8.795063 秒） | 文が byte で一致（15 本）。15 本の合算の率が `off` 以下 | 15 本とも byte で一致し、合算の率が `off` の合算以下（specified、RP-91。今の 3.8 % は scorecard の平均で、この test の合算とは数え方が違う記録なので、値の合否には使わない） | 陽性 | `rp91_a_buffer_within_the_cap_reads_the_same_as_before` |
| T-RP-91-002 | 実走：15 本を `C` = 8.8 秒（140800 標本）で読む | 最長の clip（140721 標本）が `C` に入る境界の内側（79 標本の余白） | 長さ ≤ `C` | 15 本とも byte で一致（specified、RP-91。derived：8.8 × 16000 = 140800 ≥ 140721） | 境界、陽性 | `a_cap_just_above_the_longest_clip_changes_nothing` |
| T-RP-91-003 | 実走：15 本を `C` = 8.79 秒（140640 標本）で読む | 最長の clip が `C` を超える境界の外側（81 標本）。`ja_10` だけが超える | 長さ ≤ `C` の clip だけを比べる | 長さが 140640 標本以下の 14 本が byte で一致し、`ja_10` は比べる集合から外れて「`C` を超える clip 1 本」と数えて出る（derived：140721 > 140640 で、ほかの 14 本は 135651 標本以下。pseudo：`ffprobe` の標本の数）。`ja_10` の文が変わるかは問わない | 境界、陽性 | `the_clips_within_an_8_79_second_cap_read_the_same_and_the_longer_one_is_counted_out` |
| T-RP-91-004 | 実走：15 本を `C` = 2 秒（32000 標本）で読む | 15 本とも `C` を超える（最短は 49771 標本）。検出器が見える側 | 少なくとも 1 本の文が変わる | 15 本のうち 1 本以上で文が `off` と違う（specified、D 6 の RP-91 の陽性の列。derived：全部が 2 片以上に分かれる）。001 が byte の比べを 0 件の差で通す側の対で、この行は比べが差を見られることを示す | 陰性 | `a_two_second_cap_changes_a_short_clip` |
| T-RP-91-005 | 「`C` を超える clip の数」を出す関数に、15 本の標本数と `C` = 10 秒、`C` = 8.8 秒、`C` = 8.79 秒、`C` = 2 秒 | 数が 0、0、1、15 になる 4 つの `C` | 数を出す | `C` = 10 秒で 0、8.8 秒で 0、8.79 秒で 1（`ja_10`）、2 秒で 15（derived：標本数の比べ。D 6 の陰性の列「`C` を超える clip が無いことを数えて出す」。2 秒で 15 が出ることが、0 を「数えられない」と読まない陽性対照） | 境界、陽性 | `the_clips_longer_than_the_cap_are_counted_and_printed` |
| T-RP-91-006 | 判定の関数に、上限ありの合算の率 `off` と同じ値 | 率の比べの境界の内側（等しい） | 上限あり ≤ `off` | `Ok`（specified、RP-91 の「`off` 以下」。等しいは通る） | 境界、陽性 | `a_capped_error_equal_to_the_uncapped_error_passes_for_short_clips` |
| T-RP-91-007 | 判定の関数に、上限ありの合算の率を `off` より 1 micro 大きくする | 率の比べの境界の外側（1 micro 外） | 上限あり ≤ `off` | `Err` で、1 micro 悪いことを言う（specified、同上。006 が対の陽性） | 境界、陰性 | `a_capped_error_one_millionth_above_the_uncapped_error_is_rejected` |
| T-RP-92-001 | 受入：T = 25 秒（400000 標本）、`C` = 10 秒、N = 3。tone に zero の枠を 2 つ：枠 440（標本 140800 から。的 133333.3 の +7466.7 標本）と枠 840（標本 268800 から。的 266666.7 の +2133.3 標本） | 谷が的の ±`W/2` の内側（D 6 の合成の音声）。的が整数にならない長さ | 片 3、各切れ目が谷の枠の近く、各片 ≤ `C` | 片が 3 つ。切れ目 1 が 140800、切れ目 2 が 268800（谷の枠の先頭の標本）。片の長さは 140800、128000、131200 で、3 つとも 160000 以下。片を順に繋ぐと入力と一致（specified、RP-92 と D 3.22 の切り方の決まり 1〜3。derived：的は floor(400000 / 3) = 133333 と floor(800000 / 3) = 266666。切れ目 1 の窓は max(117333, 80000, 320) 以上 min(149333, 160000) 未満、切れ目 2 の窓は max(250666, 240000, 141120) 以上 min(282666, 300800) 未満で、枠 440 と枠 840 の先頭がそれぞれの窓に入る。pseudo：shell の算術） | 陽性 | `a_long_buffer_is_cut_at_its_quietest_frames_into_balanced_pieces` |
| T-RP-92-002 | F24。tone に zero の枠 420（標本 134400 から。的 128000 の +6400 標本 = +0.4 秒）と枠 830（標本 265600 から。的 256000 の +9600 標本 = +0.6 秒） | 谷が的より後ろに在る（的を決め打つ build を外す） | 切れ目が谷の枠に在る | 片が 3 つ。切れ目 1 が 134400、切れ目 2 が 265600。片の長さは 134400、131200、118400 標本（8.4、8.2、7.4 秒）で、全部 160000 以下（derived：窓 1 は [112000, 144000)、窓 2 は max(240000, 224000, 134720) 以上 min(272000, 294400) 未満で、どちらも谷の枠の先頭を含む。pseudo：shell の算術）。的 128000 と 256000 で切る build は、切れ目が谷の枠に無くて落ちる | 陽性 | `cuts_land_in_valleys_after_the_target` |
| T-RP-92-003 | F24。tone に zero の枠 370（標本 118400 から。的の −9600 標本）と枠 780（標本 249600 から。的の −6400 標本） | 谷が的より前に在る（002 の鏡。いつも後ろへ寄せる build を外す） | 切れ目が谷の枠に在る | 切れ目 1 が 118400、切れ目 2 が 249600。片の長さは 118400、131200、134400 標本（derived：002 の鏡。窓 2 は 240000 以上 min(272000, 278400) 未満） | 陽性 | `cuts_land_in_valleys_before_the_target` |
| T-RP-92-004 | F16。tone に dip の枠 380（標本 121600 から。窓の中）と zero の枠 480（標本 153600 から。的の +25600 標本 = +1.6 秒で窓の外） | 一番静かな枠が窓の外に在る | 窓の中の最も静かな枠で切る | 切れ目が 121600（specified、D 6 の RP-92 の陽性の列「谷を的の外に置くと、切れ目が的の ±`W/2` の中の最も静かな枠に来る」）。窓の外の zero に引かれる build は、切れ目が 153600 に来て落ちる | 陽性 | `a_valley_outside_the_search_window_is_ignored` |
| T-RP-92-005 | F16。004 の zero の枠を枠 440（標本 140800 から。窓の中）へ動かす。dip の枠 380 はそのまま | 004 の対。静かな枠が窓の中に在る | 窓の中の最も静かな枠で切る | 切れ目が 140800（specified、同上。zero は dip より静かなので dip の枠 380 には来ない。004 の期待が「窓の外を無視する」ことで決まっていて、「dip を好む」ことで決まっていないことを示す対照） | 陽性 | `a_valley_inside_the_search_window_is_found` |
| T-RP-92-006 | F16。tone に zero の枠 449（標本 143680 から 144000 の手前） | 窓の高い側の端、窓に入る最後の枠 | 窓は [112000, 144000)、枠は 350 以上 450 未満 | 切れ目が 143680（specified、D 3.22 の切り方の決まり 3。derived：`hi` = min(144000, 160000) = 144000 = 450 × 320 で、枠 449 の先頭 143680 < 144000） | 境界、陽性 | `the_last_frame_inside_the_window_is_found` |
| T-RP-92-007 | F16。tone に zero の枠 450（標本 144000 から。`hi` を先頭に持つ枠）と dip の枠 400（標本 128000 から。窓の中） | 窓の高い側の端の外側。`hi` を先頭に持つ枠は入らない（最小の刻み 1 枠） | 窓の中だけを探す | 切れ目が 128000（specified、D 3.22 の切り方の決まり 3「窓の高い側の端 `hi_k` を先頭に持つ枠は入らない」）。端を含めて探す build は枠 450 に来て落ちる。006 が対の陽性 | 境界、陰性 | `a_frame_beyond_the_window_is_ignored_at_the_high_end` |
| T-RP-92-008 | F16。tone に zero の枠 350（標本 112000 から） | 窓の低い側の端、窓に入る最初の枠。`lo` を先頭に持つ枠は入る | 窓は [112000, 144000)、枠は 350 以上 450 未満 | 切れ目が 112000（specified、同上。derived：`lo` = max(112000, 96000, 320) = 112000 = 350 × 320） | 境界、陽性 | `the_first_frame_inside_the_window_is_found` |
| T-RP-92-009 | F16。tone に zero の枠 349（標本 111680 から。`lo` の 1 枠前）と dip の枠 400 | 窓の低い側の端の外側（最小の刻み 1 枠） | 窓の中だけを探す | 切れ目が 128000（specified、同上。窓を 1 枠広く探す build は枠 349 に来て落ちる。008 が対の陽性） | 境界、陰性 | `a_frame_beyond_the_window_is_ignored_at_the_low_end` |
| T-RP-92-010 | tone の T = 160000 標本（`C` ちょうど） | `T ≤ C` の境界の上端 | 片の数 1 | 片が 1 つで、標本が入力と 1 つ残らず一致する（specified、RP-92 の「`T ≤ C` は 1 片で入力と同じ」） | 境界、陽性 | `a_buffer_of_exactly_the_cap_is_one_piece` |
| T-RP-92-011 | tone の T = 160001 標本（`C` + 1 標本） | `T > C` の境界の下端。標本 1 つ分の超過 | N = ceil(T / C) = 2 | 片が 2 つで、切れ目が 80000。長さは 80000 と 80001（derived：的 floor(160001 / 2) = 80000 = 250 × 320 で、全部の枠が同値なので的に最も近い枠 250 の先頭。窓は [64000, 96000)。標本ではなく枠の数で N を決める build は、500 枠で 1 片と読んで落ちる） | 境界、陽性 | `a_buffer_one_sample_over_the_cap_is_two_pieces` |
| T-RP-92-012 | tone の T = 160320 標本（`C` + 1 枠 = 20 ms） | `T > C` の境界。1 枠分の超過 | N = 2 | 片が 2 つで、切れ目が 80000。長さは 80000 と 80320（derived：的 80160 は枠 250（先頭 80000）と枠 251（先頭 80320）から等しく 160 標本。同じ距離なら前の枠を採る決まり 5 で 80000。後の枠を採る build は 80320 で落ちる。011 と合わせて、標本と枠の両方の刻みで N を決める） | 境界、陽性 | `a_buffer_one_frame_over_the_cap_is_two_pieces` |
| T-RP-92-013 | tone の T = 320000 標本（`2C` ちょうど） | N が 2 から 3 に変わる境界の内側 | N = ceil(T / C) = 2 | 片が 2 つで、切れ目が 160000、長さが 160000 と 160000（derived：2.0 の ceil は 2。浮動小数の割り算で 2.0000001 になる build は 3 片にして落ちる。窓は `lo` = max(144000, 160000, 320) = 160000、`hi` = min(176000, 160000) = 160000 で空になり、決まり 3 で切れ目は `lo` = `C` に置かれる。決まり 4 のとおり、どちらの片も `C` 以下） | 境界、陽性 | `a_buffer_of_exactly_twice_the_cap_is_two_pieces` |
| T-RP-92-014 | tone の T = 320001 標本（`2C` + 1 標本） | N が 3 に変わる境界の外側 | N = 3 | 片が 3 つで、切れ目が 106560 と 213440。長さは 106560、106880、106561（derived：N = ceil(2.0000063) = 3。的 floor(320001 / 3) = 106667 は枠 333（先頭 106560、距離 107）と枠 334（先頭 106880、距離 213）の間で近い方、的 213334 は枠 666（213120、距離 214）と枠 667（213440、距離 106）の間で近い方。pseudo：決まり 1〜5 を素朴に全探索する awk を打った）。013 が対 | 境界、陽性 | `a_buffer_one_sample_over_twice_the_cap_is_three_pieces` |
| T-RP-92-015 | tone の T = 320320 標本（`2C` + 1 枠） | N が 3 に変わる境界。1 枠分の超過 | N = 3 | 片が 3 つで、切れ目が 106880 と 213440。長さは 106880、106560、106880（derived：的 floor(320320 / 3) = 106773 は枠 334（106880、距離 107）が近く、的 213546 は枠 667（213440、距離 106）が近い。pseudo：同上の awk。1001 枠の buffer）。014 と合わせて、標本と枠の両方の刻み | 境界、陽性 | `a_buffer_one_frame_over_twice_the_cap_is_three_pieces` |
| T-RP-92-016 | tone だけ（無音なし、全部の枠が同じ RMS）の T = 400000 標本（25 秒）、`C` = 10 秒 | 谷が無い音声。全部の枠が同値 | 片の数 = ceil(T / C)、各片 ≤ `C` | 片が 3 つで、切れ目が 133440 と 266560。長さは 133440、133120、133440 で、3 つとも 160000 以下。片を順に繋ぐと入力と一致（specified、D 6 の RP-92 の陰性の列と決まり 4・5。derived：的 133333 と 266666 に最も近い枠の先頭。窓は [117333, 149333) と [250666, 282666)。pseudo：同上の awk） | 境界、陰性 | `a_buffer_without_silence_is_cut_into_pieces_within_the_cap` |
| T-RP-92-017 | T = 25 秒の tone に zero の枠 2 つ（001 の buffer）を、上限 `off` で。同じ buffer を、Rz 以外の tier で `C` = 10 秒で。別に、上限 `C` = 10 秒の Rz で同じ buffer | `off` の側、Rz 以外の側、上限が効く側（陽性対照） | 片の数 1 | `off` と Rz 以外の tier は片が 1 つで、標本が入力と一致。上限 10 秒の Rz は片が 3 つ（specified、D 6 の RP-92 の「`off` と `T ≤ C` は 1 片で入力と同じ。Rz 以外の tier は切らない」。3 つ目が、1 片になる理由が buffer の短さでないことを示す対照）。分岐を tier の側に持たせるか cap.rs の入口に持たせるかは実装の席が決める | 陰性 | `an_uncapped_or_other_tier_buffer_is_one_piece` |
| T-RP-92-018 | 一様な雑音（splitmix64、seed `0x9E3779B97F4A7C15`、値は −0.5 以上 0.5 未満）の T = 160001、200000、250001、320001、400000、450001 標本。`C` = 160000、`W` = 32000 | 片の数の範囲 2 と 3 を通る性質 | 性質 4 つ | 6 つの T のすべてで、片の数が ceil(T / C)（2、2、2、3、3、3）。片を順に繋ぐと入力と一致。どの片も 320 標本以上 160000 標本以下。切れ目が 320 の倍数。切れ目が、決まり 1〜5 を素朴に全探索で書いた別の関数（枠ごとに 2 乗の和を f64 で出し、窓の中で最小、的に最も近い、前の枠の順に選ぶ）の切れ目と標本まで一致する（pseudo。property。落ちた入力は最小にして example の test に写す） | 性質 | `pieces_tile_the_buffer_and_each_cut_stays_near_its_target` |
| T-RP-92-019 | F16。tone に zero の枠 380（先頭 121600、的 128000 から 6400 前）と枠 420（先頭 134400、的から 6400 後） | 同じ RMS で、的から等しい距離に 2 つ | 同じ距離なら前の枠 | 切れ目が 121600（specified、D 3.22 の切り方の決まり 5「距離も同じなら、前の枠を採る」）。後の枠を採る build は 134400 で落ちる | 境界、陽性 | `two_equally_distant_quiet_frames_give_the_earlier_one` |
| T-RP-92-020 | F16。（a）tone に zero の枠 390（先頭 124800、距離 3200）と枠 415（先頭 132800、距離 4800）。（b）tone に zero の枠 385（先頭 123200、距離 4800）と枠 405（先頭 129600、距離 1600） | 同じ RMS で、距離が違う 2 つ。近い方が前の場合と後の場合 | 的に最も近い枠 | （a）は 124800、（b）は 129600（specified、決まり 5「的 `t_k` との距離（`s` と `t_k` の差の絶対値）が最も小さい枠」。いつも最初の枠を採る build は（b）で 123200 になり、いつも最後の枠を採る build は（a）で 132800 になって落ちる） | 境界、陽性 | `the_quiet_frame_nearest_the_target_is_taken_whichever_side_it_is_on` |
| T-RP-92-021 | F16。tone に zero の枠 380 から 420 までの 41 枠（先頭 121600 から 134720 の手前。無音が何枠も続く） | 同じ RMS の枠が連なる無音 | 的に最も近い無音の枠 | 切れ目が 128000（枠 400 の先頭。的 128000 との距離 0）（specified、決まり 5「無音が何枠も続く所では、的に最も近い無音の枠で切る」） | 陽性 | `a_long_silence_is_cut_at_the_frame_nearest_the_target` |
| T-RP-92-022 | tone の T = 319680 標本（`2C` − 1 枠 = 999 枠）に zero の枠 450（先頭 144000）。N = 2、的 floor(319680 / 2) = 159840 | `lo` の 2 つ目の項 T − (N − k)·C が効く境。窓が 1 枠に縮む | どの片も `C` 以下 | 切れ目が 159680、長さが 159680 と 160000（specified、決まり 3・4。derived：`lo` = max(143840, 159680, 320) = 159680、`hi` = min(175840, 160000) = 160000 で、先頭がこの区間に入る枠は枠 499（159680）だけ。zero の枠 450 を採る build は 2 つ目の片が 175680 になって落ちる） | 境界、陰性 | `a_valley_that_would_leave_a_piece_over_the_cap_is_not_taken` |
| T-RP-92-023 | tone の T = 400000 標本（25 秒）に zero の枠 367（先頭 117440）と枠 870（先頭 278400）。N = 3 | `hi` の 2 つ目の項 `cut_{k−1}` + `C` が効く境（切れ目 1 が早いと窓 2 の上端が 277440 に下がる） | どの片も `C` 以下 | 切れ目が 117440 と 266560。長さが 117440、149120、133440（specified、決まり 3・4。derived：窓 2 は `lo` = max(250666, 240000, 117760) = 250666、`hi` = min(282666, 117440 + 160000) = 277440 で、枠 870 の先頭 278400 は外。残る枠は同値なので的 266666 に最も近い枠 833（266560）。枠 870 を採る build は 2 つ目の片が 160960 になって落ちる） | 境界、陰性 | `a_valley_beyond_the_previous_cut_plus_the_cap_is_not_taken` |
| T-RP-92-024 | F24 の 002 の buffer（枠 420 と枠 830 が zero）、`O` = 8000 標本（0.5 秒） | 重ね `O` > 0。片 `k` ≥ 2 は `cut_{k−1} − O` から始まる | 片の長さは最長 `C` + `O` | 3 つの片が [0, 134400)、[126400, 265600)、[257600, 384000)。長さは 134400、139200、126400 で、全部 168000 以下。`O` = 0 の 002 と切れ目は同じ（specified、決まり 7。決まり 4 の保証は `O` を除いた部分に当たる） | 境界、陽性 | `with_an_overlap_each_later_piece_starts_one_overlap_before_its_cut` |
| T-RP-93-001 | 受入：LB-3 が書いた `docs/results/long_buffer/sweep.tsv` と、`hayamimi config dump` が出す上限の行 | 実物。選んだ印が 1 行 | 選んだ行 = 資格のある行の `C` の最大（同点は `O` = 0）。dump は `off` か選んだ行 | `Ok`（specified、RP-93。LB-3 の席が `sweep.tsv` を書いてから通る） | 陽性 | `rp93_the_default_cap_is_the_one_the_sweep_chose` |
| T-RP-93-002 | F93a。選んだ印は (10, 0)。dump の上限は `off` | 既定を替える commit の前 | dump ∈ {`off`、選んだ行} | `Ok`（specified、RP-93「前は `off`」） | 陽性 | `a_default_still_off_before_the_default_changes_is_accepted` |
| T-RP-93-003 | F93a。選んだ印は (10, 0)。dump の上限は `C` = 10、`O` = 0 | 既定を替えた commit の後 | dump ∈ {`off`、選んだ行} | `Ok`（specified、RP-93「後は選んだ行の `C` と `O`」。002 と合わせて、前後の両方を通す） | 陽性 | `a_default_equal_to_the_chosen_row_is_accepted` |
| T-RP-93-004 | F93a。選んだ印は (10, 0)。dump の上限は `C` = 11、`O` = 0 | dump が選んだ行から 1 秒ずれる | dump ∈ {`off`、選んだ行} | `Err` で、dump の 11 と選んだ行の 10 を名指す（specified、D 7 の陰性の case「1 秒ずらした dump」）。003 が対の陽性 | 境界、陰性 | `a_shifted_default_is_rejected` |
| T-RP-93-005 | F93a。選んだ印は (10, 0)。dump の上限は `C` = 10、`O` = 0.5 | dump の `O` だけがずれる | 同上 | `Err` で、`O` が 0.5 で 0 でないことを言う（specified、RP-93「`C` と `O` に一致する」） | 境界、陰性 | `a_default_overlap_that_differs_from_the_chosen_one_is_rejected` |
| T-RP-93-006 | F93a。選んだ印を (11, 0) に置く（行が無い） | 選んだ行が 1 秒ずれて、grid に無い | 選んだ行 = 資格のある行 | `Err` で、(11, 0) の行が `sweep.tsv` に無いことを言う（specified、D 6 の RP-93 の陽性の列「選んだ行を 1 秒ずらした `sweep.tsv`」） | 境界、陰性 | `a_chosen_row_one_second_off_the_grid_is_rejected` |
| T-RP-93-007 | F93a。選んだ印を (8, 0) に置く | 資格のある行だが `C` の最大でない | `C` の最大 | `Err` で、`C` = 10 の行が資格を持つことを言う（specified、RP-93「`C` の最も大きい物」。最大でなく最初の資格のある行を選ぶ build は落ちる）。002 が対の陽性 | 境界、陰性 | `a_smaller_cap_than_the_largest_eligible_one_is_rejected` |
| T-RP-93-008 | F93a。選んだ印を (10, 0.5) に置く | `C` が同点で `O` の大きい方 | 同点は `O` = 0 | `Err` で、(10, 0) も資格を持つので `O` = 0 を選ぶことを言う（specified、RP-93「同点は `O` = 0」。D 3.22 の方針 1「`O` = 0 が同点なら `O` = 0 を採る」） | 境界、陰性 | `a_tie_on_the_cap_is_broken_towards_zero_overlap` |
| T-RP-93-009 | F93b。選んだ印は (10, 0.5) | `O` = 0.5 だけが通る（同点でない） | 資格のある行の `C` の最大 | `Ok`（specified、RP-93。`O` = 0 を無条件に好む build は (10, 0) が落ちる行を選んで落ちる。008 が同点の側の対） | 境界、陽性 | `an_overlap_is_chosen_when_it_is_the_only_one_that_passes` |
| T-RP-93-010 | F93c。選んだ印は (10, 0)。別に、選んだ印を (8, 0) に置く | 1 つの経路（refine）の RP-91 だけが落ちる行 | 全部の経路で通る | (10, 0) は `Err` で、refine の RP-91 が落ちることを言う。(8, 0) は `Ok`（specified、RP-93「全部の経路で RP-90 と RP-91 を通り」。経路の和集合や live だけで見る build は (10, 0) を通して落ちる） | 境界、陰性 | `a_cap_failing_on_one_route_is_not_eligible` |
| T-RP-93-011 | F93d。選んだ印は (10, 0)。別に、選んだ印を (8, 0) に置く | RP-90 と RP-91 は通り、RP-94 だけが落ちる行 | RP-94 も通る | (10, 0) は `Err` で、RP-94 が落ちることを言う。(8, 0) は `Ok`（specified、RP-93「RP-94 も通した組」） | 境界、陰性 | `a_cap_failing_rp94_is_not_eligible` |
| T-RP-93-012 | F93a に `off` の行を足し、その行に選んだ印を置く | `off` は候補でない。`C` を無限と読むと最大になる | `off` を選ばない | `Err` で、`off` の行は候補でないことを言う（specified、RP-93「`C` の最も大きい物」の `C` は秒の値。derived：D 3.22 の掃引は `off` を基準の腕に置く）。003 が対の陽性 | 境界、陰性 | `the_off_row_is_never_the_chosen_row` |
| T-RP-93-013 | F93e。選んだ印を (10, 0) に置く。別に、選んだ印を置かない | 資格のある行が 0 | 選ぶ行が在る | どちらも `Err` で、資格のある行が無いことを言う。0 件を通さない（implicit。D 3.22 の「共通部分が空なら評議の席へ出す」） | 境界、陰性 | `an_empty_set_of_eligible_rows_is_not_a_pass` |
| T-RP-93-014 | F93a。選んだ印を 2 行に置く。別に、選んだ印を置かない | 選んだ印が 2 つ、0 個 | 選んだ印は 1 つ | どちらも `Err` で、選んだ印の数を言う（specified、RP-93「選んだ行」。002 が 1 つの陽性） | 境界、陰性 | `a_sweep_must_carry_exactly_one_chosen_mark` |
| T-RP-93-015 | `docs/results/long_buffer/sweep.tsv` を置かない run | file が無い | file が在る | `Err` で、`sweep.tsv` の path を名指す。飛ばして通さない（specified、D 6 の RP-93 の陰性の列「`sweep.tsv` が無いか」） | 境界、陰性 | `a_missing_sweep_names_its_path` |
| T-RP-93-016 | F93a の `sweep.tsv`。14 の列を 1 つずつ外した形を、14 通り作る | 列が 1 つ欠ける（列ごとに） | 全部の列が在る | どの形も `Err` で、`sweep.tsv` の path と、外した列の名前を名指す（specified、D 6 の RP-93 の陰性の列「列が欠けると、path と列を名指して落ちる」。F93a の全部の列が在る形が `Ok` の陽性対照） | 境界、陰性 | `a_sweep_without_a_column_is_named` |
| T-RP-93-017 | 列の見出しだけで行が 0 の `sweep.tsv` | 行が 0 | 行が在る | `Err` で、行が無いことを言う。0 行を「全部通った」と読まない（implicit） | 境界、陰性 | `a_sweep_with_no_rows_is_rejected` |
| T-RP-93-018 | F93a と、上限の行を持たない dump の text | dump に上限の key が無い | key が在る | `Err` で、上限の key を名指す（implicit。key の綴りは LB-1 の席が決め、test は定数 1 か所に持つ。003 が key の在る陽性） | 境界、陰性 | `a_dump_without_the_cap_names_the_key` |
| T-RP-93-019 | F93a で、（a）(10, 0) の `live_rp90` を `-` にして選んだ印を (10, 0) に置く。（b）(10, 0) の `rp94` を `-` にして同じ印 | `-`（測っていない）は通っていない側 | 資格のある行は全部 `pass` | どちらも `Err` で、(10, 0) が資格を持たないことを言う（specified、D 3.22 の `sweep.tsv` の形「`-` は通っていない側に数える」。`-` を通ったと読む build は落ちる。002 が対の陽性） | 境界、陰性 | `an_unmeasured_cell_does_not_count_as_a_pass` |
| T-RP-93-020 | F93a の 1 行を 13 欄にした `sweep.tsv` | 欄の数が 14 でない行 | 全部の行が 14 欄 | `Err` で、`sweep.tsv` の path とその行の番号を名指す（implicit。列の数が合わない行を読み飛ばさない） | 境界、陰性 | `a_row_with_the_wrong_number_of_fields_is_named` |
| T-RP-93-021 | F93a の `off` の行の `overlap_s` が `-`、`chosen` が 0 | `off` の行の形 | `off` は `overlap_s` が `-` | `Ok`。`off` の `overlap_s` を数として読もうとして落ちる build は、この行で落ちる（specified、D 3.22 の `sweep.tsv` の形） | 境界、陽性 | `the_off_row_carries_a_dash_for_the_overlap` |
| T-RP-94-001 | 受入：実走（窓 E4 だけ）。長い材料 6 本について、`off` と候補の `C` を交互に 5 回ずつ（off、cap、off、cap、…の順に 10 回）復号し、復号の時間（ms）を数える。始める前と終わった後の `load1` を記録する | 5 回の対の差 `d_i` = cap の時間 − off の時間（i = 1〜5） | 6 本それぞれで、`d` の p50 ≤ 150 ms、p95 ≤ 250 ms（nearest-rank、n = 5）。始める前の `load1` < 2.0 | 6 本とも 2 つの線の内（specified、RP-94）。n = 5 の p50 は小さい方から 3 番目、p95 は 5 番目 = 最大（derived：T 4.1 の `(p × n + 99) / 100`）。run は 6 本それぞれの `d` の 5 つの値と p50、p95、A の式の値（`C` ごと）を並べて出す | 陽性 | `rp94_a_capped_decode_adds_at_most_150_ms_p50_and_250_ms_p95` |
| T-RP-94-002 | 時間の線の関数に、`d` = 250、10、40、20、30 ms（順は不同） | p95 の線の境界の内側。n = 5 の p95 は最大 | p95 ≤ 250 ms | p95 は 250 ms で、p95 の線は通る（specified、RP-94。nearest-rank の 5 番目。derived：`(95 × 5 + 99) / 100` = 5。`percentile.rs:21` の `nearest_rank` と同じ式）。p50 は 30 ms | 境界、陽性 | `a_p95_of_250_ms_is_on_the_line` |
| T-RP-94-003 | 時間の線の関数に、`d` = 251、10、40、20、30 ms | p95 の線の境界の外側（1 ms 外） | p95 ≤ 250 ms | `Err` で、p95 が 251 ms で 250 ms の線を超えることを言う（specified、同上。002 が対の陽性） | 境界、陰性 | `a_p95_of_251_ms_is_over_the_line` |
| T-RP-94-004 | 時間の線の関数に、`d` = 10、20、150、200、240 ms | p50 の線の境界の内側。n = 5 の p50 は 3 番目 | p50 ≤ 150 ms | p50 は 150 ms で通る。p95 は 240 ms で通る（specified、RP-94。derived：`(50 × 5 + 99) / 100` = 3） | 境界、陽性 | `a_median_of_150_ms_is_on_the_line` |
| T-RP-94-005 | 時間の線の関数に、`d` = 10、20、151、200、240 ms | p50 の線の境界の外側（1 ms 外） | p50 ≤ 150 ms | `Err` で、p50 が 151 ms で 150 ms の線を超えることを言う。p95 は線の内（specified、同上。004 が対の陽性で、違いは 3 番目の値だけ） | 境界、陰性 | `a_median_of_151_ms_is_over_the_line` |
| T-RP-94-006 | 時間の線の関数に、`d` = 10、20、30、40、300 ms | 順位を 1 つ下げる誤り（n = 5 の p95 を 4 番目と読む build を外す） | p95 は最大 | `Err` で、p95 が 300 ms で線を超えることを言う（derived：5 番目 = 300 ms。4 番目 = 40 ms を p95 と読む build は通して落ちる。T 4.84 の RP-61-003 と同じ型の対照） | 境界、陰性 | `the_p95_of_five_samples_is_the_largest_one` |
| T-RP-94-007 | 時間の線の関数に、`d` = 124.44 ms を 5 つ（`C` = 6 秒の式の値）。別に 198.16 ms を 5 つ（`C` = 4 秒の式の値） | 線 150 ms が式の値の間に在る（D 6 の「`C` = 4 秒を外す」） | p50 ≤ 150 ms | 124.44 ms の 5 つは `Ok`。198.16 ms の 5 つは `Err` で p50 の線を超えることを言い、p95 の線（250 ms）は内（derived：D 3.22 の遅れの式 A。pseudo：`awk` で `23.0 × (N − 1) + 19.8 × 0.7 × N` を N = 4 と 6 で打った。µs の整数で比べる） | 境界、陽性、陰性 | `the_formula_value_for_four_pieces_passes_and_six_pieces_fails_the_median_line` |
| T-RP-94-008 | 式の関数に N = 1、2、3、4、6、11 | N = 1 は片に分けない。N = 2 以上は分ける | 0、50.72、87.58、124.44、198.16、382.46 ms | N = 1 は 0 ms。N = 2 は 50_720 µs、N = 3 は 87_580 µs、N = 4 は 124_440 µs、N = 6 は 198_160 µs、N = 11 は 382_460 µs（specified、D 3.22 の遅れの式 A の `ΔL_dec = a·(N − 1) + b·2p·N`、`a` = 23.0 ms、`b` = 19.8 ms/秒、`p` = 0.35 秒。pseudo：`awk` で打った。N = 1 に式をそのまま当てると 13.86 ms になる誤りを、0 の期待が外す）。長い材料 6 本の N は、`C` = 14、10、8、6、4、2 秒で 2、3、3、4、6、11（pseudo：`ceil(T / C)`、T は 20.07〜21.19 秒） | 境界 | `the_delay_formula_gives_zero_for_one_piece_and_the_design_values_for_more` |
| T-RP-94-009 | 始める前の `load1` を 1.99 とした記録 | `load1` < 2.0 の境界の内側（100 分の 1 の刻み） | 始める前の `load1` < 2.00 | 記録が有効（specified、RP-94「2.0 未満で始める」。整数の 199 と 200 で比べる） | 境界、陽性 | `a_run_that_starts_at_load_1_99_is_valid` |
| T-RP-94-010 | 始める前の `load1` を 2.00 とした記録。別に、終わった後の `load1` が無い記録 | 境界の外側（ちょうど。「未満」なので外）。終わった後の記録の欠け | 両方の `load1` が在り、始めが 2.00 未満 | どちらも `Err` で、前の記録は 2.00 を、後の記録は終わった後の値が無いことを名指す（specified、同上。009 が対の陽性） | 境界、陰性 | `a_run_that_starts_at_load_2_00_or_lacks_the_end_load_is_invalid` |
| T-RP-94-011 | 測った順の記録が、off、cap、off、cap、…の 10 回。別に、off が 5 回続いてから cap が 5 回。別に、off が 4 回と cap が 6 回 | 交互、並びが交互でない、回数が 5 回ずつでない | 交互に 5 回ずつ | 交互の 10 回は `Ok`。後の 2 つは `Err` で、並びまたは回数を言う（specified、RP-94「交互に 5 回ずつ」。続けて測る形は熱や cache の流れを差に混ぜる） | 境界、陰性 | `a_run_that_does_not_alternate_five_and_five_is_rejected` |
| T-RP-94-012 | 実走：窓 E4。001 と同じ 6 本を `C` = 2 秒で測る | 線を超える側（陽性対照） | 001 と同じ線 | 時間の線の関数が `Err` で、p50 と p95 の両方の線を超える（specified、D 7 の陰性の case。derived：N = 11 の式の値は 382.46 ms で、150 ms と 250 ms の両方より大きい）。実測が式の半分から 2 倍の外なら、線を引き直して評議の席へ出す（D 6 の RP-94） | 陰性 | `a_two_second_cap_exceeds_the_line` |
| T-RP-95-001 | 受入：実走（窓 E4 だけ）。lat-ja の E2E-1（RP-32 の交互の形）を、同じ build の `--decode-cap off`（明示）と `--decode-cap <選んだ C>` で交互に流す。基準は `docs/results/r8x/e3q1.e2e1`、`e3q2.e2e1` と `docs/results/rp32/` | lat-ja の 23 発話 | 差 `L` の p50 ≤ 31 ms、p95 ≤ 100 ms。訳の取れた対 ≥ 20 / 23。cap の run の `L` の p95 ≤ 1600 ms | 3 つの線が全部通る（specified、RP-95。差は cap の run − `off` の run、p50 と p95 は nearest-rank）。run は差、対の数、`C` を超えた発話の数を出力に並べる | 陽性 | `rp95_the_cap_does_not_delay_the_subtitle` |
| T-RP-95-002 | 遅れの関数に、p50 の差 31 ms | p50 の線の境界の内側 | 差 ≤ 31 ms | p50 の判定が通る（specified、RP-95。他の判定は通る値を置く） | 境界、陽性 | `a_median_delay_of_31_ms_is_on_the_line` |
| T-RP-95-003 | 遅れの関数に、p50 の差 32 ms | p50 の線の境界の外側（1 ms 外） | 差 ≤ 31 ms | `Err` で、差が 32 ms で 31 ms の線を超えることを言う（specified、同上。002 が対の陽性） | 境界、陰性 | `a_median_delay_of_32_ms_is_over_the_line` |
| T-RP-95-004 | 遅れの関数に、p95 の差 100 ms | p95 の線の境界の内側 | 差 ≤ 100 ms | p95 の判定が通る（specified、同上） | 境界、陽性 | `a_p95_delay_of_100_ms_is_on_the_line` |
| T-RP-95-005 | 遅れの関数に、p95 の差 101 ms | p95 の線の境界の外側（1 ms 外） | 差 ≤ 100 ms | `Err` で、差が 101 ms で 100 ms の線を超えることを言う（specified、同上。004 が対の陽性） | 境界、陰性 | `a_p95_delay_of_101_ms_is_over_the_line` |
| T-RP-95-006 | 遅れの関数に、訳の取れた対 20 / 23 | 対の数の境界の内側 | 対 ≥ 20 | 対の判定が通る（specified、RP-95「20 / 23 以上」） | 境界、陽性 | `20_of_23_translated_pairs_is_enough` |
| T-RP-95-007 | 遅れの関数に、訳の取れた対 19 / 23 | 対の数の境界の外側（1 対外） | 対 ≥ 20 | `Err` で、19 対で 20 対に足りないことを言う（specified、同上。006 が対の陽性） | 境界、陰性 | `19_of_23_translated_pairs_is_not_enough` |
| T-RP-95-008 | 遅れの関数に、cap の run の `L` の p95 が 1600 ms。別に 1601 ms | RP-67 の 1600 ms の線の境界の両側 | p95 ≤ 1600 ms | 1600 ms は通り、1601 ms は `Err` で線を超えることを言う（specified、RP-95「RP-67 の 1600 ms の線を保つ」と RP-67 の 1600 ms 以下） | 境界、陽性、陰性 | `a_cap_run_p95_of_1600_ms_keeps_the_rp67_line_and_1601_ms_does_not` |
| T-RP-95-009 | 遅れの関数に、p50 の差 −50 ms、p95 の差 −50 ms（cap の run が速い） | 悪化の向き。差が負 | 悪化だけを見る | `Ok`（specified、RP-95「悪化しない」。差の絶対値で見る build は 50 ms を 31 ms の線の外と読んで落ちる） | 境界、陽性 | `a_faster_capped_run_is_not_a_failure` |
| T-RP-95-010 | 実走：窓 E4。同じ build の `--decode-cap off`（明示）の run を 2 つ、001 と同じ交互の形で流す | `off` と `off` の対（物差しの床） | 差 p50 ≤ 31 ms、p95 ≤ 100 ms | 差が 2 つの幅の内（specified、D 7 の陰性の case。幅の内に入らなければ、この幅は物差しにならないので、cap の差を判定に使わず評議の席へ出す） | 陰性 | `an_off_against_off_pair_is_within_the_floor` |
| T-RP-95-011 | 2 つの arm の記録。1 つは引数に `--decode-cap off`、もう 1 つは `--decode-cap <選んだ C>` を持つ。別に、`--decode-cap` を持たない arm の記録 | 引数の明示 | 両方の arm が明示 | 2 つとも明示の記録は `Ok`。明示の無い記録は `Err` で、その arm を名指す（specified、D 10 の H158「基準の run は `--decode-cap off` を明示して取る」）。記録の形は RP-95 の席が決める | 境界、陰性 | `an_arm_without_an_explicit_cap_flag_is_rejected` |
| T-RP-95-012 | `asr.decode.chunked` の行が 2 つと、ほかの event の行を持つ log の fixture | 上限が効いた発話の数 | 行の数を数える | 数が 2（specified、D 3.22 の結論 5「`C` を超えた発話の数を数えて出す」。ほかの event の行は数えない） | 陽性 | `chunked_events_in_a_log_are_counted` |
| T-RP-95-013 | `asr.decode.chunked` の行が 0 の log の fixture。別に、log の file が無い run | 数が 0 の log、log が無い | log が在る | 0 行の log は 0 を出す。file が無い run は `Err` で path を名指し、0 と読まない（implicit。012 が数が 0 でない陽性対照。lat-ja の発話は最長 9.1 秒で `C` = 10 秒の run の数は 0 の見込み。D 3.22 の結論 5、ASSUMPTION） | 境界、陰性 | `a_log_with_no_chunked_event_counts_zero_and_a_missing_log_is_an_error` |

**この節の設計にない物**（system-architect が決める。この席は受入の条件を足さない）：

設計の席が決めた物（切り方の決まり 1〜8、`sweep.tsv` の形、H159、D 6 の呼び名）は、この一覧から外した。残るのは次の 3 つである。

1. **rp82 との相互作用**：rp82（R13-15。main にまだ無い）は、節の file が 1 つでも repo に在ると、その節の受入の行と「受入 test の名前は」の名前を全部その file の組から探す。LB-0 から LB-3 の間、この節の file は一部だけ在る期間があり、rp82 は在る file に無い名前を名指して落ちる。rp82 が `（未実装）` の扱いを持つので、持ち主の slice が merge するまでその印を付けるのが、rp82 を作る席（R13-15）の仕事になる。この節の行には付けていない。
2. **`O` > 0 で一致が無い時の繋ぎ**：決まり 7 は、2〜6 字の一致が見つからない時に、決まり 6 の空白の規則（ASCII の英数字の境だけ半角空白）で繋ぐかを書かない。T-RP-90-017 の（b）は日本語の字だけで置き、空白の規則が働かない入力にした。ASCII の英数字が境に当たる場合の繋ぎは行に置いていない。
3. **重なりの一致の比べ**：決まり 7 の「一致する字」を、決まり 6 と同じく前後の空白を除いた文の字で比べるかを書かない。T-RP-90-015 から 018 は、空白を含まない文で置いた。

### 4.86 IP-1〜IP-8（対面のモード）

**最新情報**：Rust Reference の testing attributes（https://doc.rust-lang.org/reference/attributes/testing.html）は、`#[ignore = "..."]` を付けた test も test の mode では compile され、実行だけが飛ばされ、`--ignored` を付けると走ると書く。取得 2026-10-22。

test：IP-1 は `native/hayamimi-relay/tests/ip01.rs`、IP-2 は `native/hayamimi/tests/ip02.rs`（T-IP-02-022 は `mod drained`）と `native/hayamimi-eval/src/machine.rs` の `mod tests`（今在る物に足す。T-IP-02-021）、IP-3 は `native/hayamimi-asr/tests/ip03.rs` と `native/hayamimi-asr/src/route/tests/pair.rs`（新しい file。`mod pair;` を同じ directory の `mod.rs` に足す）と `native/hayamimi-asr/src/pipeline.rs` の `mod tests`（今在る物に足す）と `native/hayamimi/src/args.rs` の `mod tests` と `native/hayamimi/src/log.rs` の `mod tests`、IP-4 は `native/hayamimi-eval/tests/ip04.rs`、IP-5 は `native/hayamimi/tests/ip05.rs` と `native/hayamimi-ui/src/route.rs` の `mod tests`、IP-6 は `native/hayamimi-eval/tests/ip06.rs`、IP-7 は `native/hayamimi/tests/ip07.rs`、IP-8 は `native/hayamimi-eval/tests/ip08.rs`。
どれも作る物で、書く席は IP-S1 が ip01.rs、ip06.rs、ip08.rs、IP-S2 が ip05.rs と route.rs、IP-S3 が ip02.rs、ip03.rs、pair.rs、pipeline.rs、args.rs、log.rs、ip04.rs、ip07.rs である（設計 10.11）。
case の `T-IP-<n>-` の `<n>` が 01 なら ip01.rs、02 なら ip02.rs、04 なら ip04.rs、06 なら ip06.rs、07 なら ip07.rs、08 なら ip08.rs に置く。
IP-3 は 001 から 015 と 025 から 033 が ip03.rs、017、018、034 が pair.rs、019 から 023 が args.rs、024 と 035 が log.rs、036 から 039 が pipeline.rs に置く（016 は欠番。設計 10.4 が `route/draft.rs` を変えないので、速報の手がかりを pair が使わない行は偽になった）。
IP-5 は 001 から 006 と 017 から 019 が ip05.rs、007 から 016 が route.rs に置く。
受入 test の名前は `ip01_one_source_is_translated_both_ways_by_line_language`、`ip02_each_dialog_utterance_is_read_in_its_own_language`、`ip03_an_unsure_identifier_falls_to_ja_or_en`、`ip04_language_choice_adds_little_to_the_wait`、`ip05_one_page_shows_both_directions`、`ip06_interleaved_join_alternates_languages`、`ip07_short_replies_are_read_in_their_own_language`、`ip08_the_own_llama_server_is_checked_on_the_port_the_run_names`。
設計は `conversation_subtitles.ja.md` の 10 節のうち、10.3 から 10.5、10.7、10.7.1、10.9、10.9.1、10.9.2、10.10 から 10.12 である（この節で「設計 10.x」と書く物はこの file の節を指す）。

入力が「実走：」で始まる行は `hayamimi` の binary と model を動かし、`#[ignore = "runs the hayamimi binary with the models, which are not in git"]` を付ける。
IP-5 の「実走：」の行は binary、中継、偽の翻訳 server、chrome-headless-shell を動かすので、`#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]` を付ける。
入力が「実物：」で始まる行は git の外の `testdata/` か run の記録を読み、`#[ignore = "reads testdata or run records, which are not in git"]` を付ける。
ほかの行は model も `testdata/` も読まず、`--ignored` なしで通る。`--skip` は足さない。
IP-8 の行は `stream` の binary と偽の `/ingest` を使い、偽の `/ingest` の port は OS に空きを選ばせる（前例は RP-83 の `FakeIngest`）。8080、8833、8835、18081、18083 を開かず、`--ignored` を付けない。
IP-3 の log.rs の行と pipeline.rs の行、IP-5 の route.rs の行、IP-8 の file の行（T-IP-08-014）は、値か git の中の file を読むだけで、port を開かない。

command：cargo は全部 `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 <対象>` の形で打つ。
対象は IP-1 が `-p hayamimi-relay --test ip01`、IP-2 が `-p hayamimi --test ip02` と `-p hayamimi-eval --lib machine`（021 の 1 本）、IP-3 が `-p hayamimi-asr --test ip03`、`-p hayamimi-asr --lib route::tests::pair`、`-p hayamimi-asr --lib pipeline::tests::a_held_candidate`（036 と 037 の 2 本）、`-p hayamimi-asr --lib pipeline::tests::a_final_made_from_a_solved_reading`（038 と 039 の 2 本）、`-p hayamimi --lib args`、`-p hayamimi --lib log`、IP-4 が `-p hayamimi-eval --test ip04`、IP-5 が `-p hayamimi --test ip05` と `-p hayamimi-ui --lib route`、IP-6 が `-p hayamimi-eval --test ip06`、IP-7 が `-p hayamimi --test ip07`、IP-8 が `-p hayamimi-eval --test ip08` である。
「実走：」の行は `cargo` の前に `sudo -n nice -n -5` を挟み、後ろに `-- --ignored` を付ける（他の席の process が結果を待つ run なので、起動の時点で優先度を上げる）。
「実物：」の行は file を読むだけなので `sudo` を挟まず、後ろに `-- --ignored` だけを付ける。
fixture の行だけを打つ時は `-- --ignored` を外す。

「実走：」の行の前に確かめること（取り合う物と、始めてよい条件）：

- port：8080、8833、8835 を他の process が持っていないこと（`ss -ltn` の出力を貼る）。test が自分で開く port は空きを OS に選ばせ、8080、8833、8835、18081、18083 を使わない。18081 と 18083 は既存の `llama-server` が持つことがある。run の席はその server を止めず、起こし直さない。
- 訳の server の port：`<訳の server の port>` は、run が使う `llama-server` が listen する port である。`--llm http://127.0.0.1:<訳の server の port>`、`llama-server` の `--port`、`stream --own-llama-port` の 3 か所に同じ値を渡す。流す前に `tr '\0' ' ' < /proc/<訳の server の pid>/cmdline` の出力を貼り、`--port` の次の語がその値であることを見せる。既存の server を使う時も同じである。
- GPU と memory：`nvidia-smi` の空き memory が、`hayamimi` の model と翻訳 server が載る量に足りること。足りなければ run を始めず、落ちた理由に memory を書く。
- load：`/proc/loadavg` の 1 分の値が 2 未満。`stream` は自分でこの線を持ち、越えていれば 1 byte も送らずに `stream.machine.refused` で止まる。
- 後始末：run で起こした process は、起こした時の `$!` の pid で止める。名前で kill しない（同じ名前の process が他の席で動いている）。

**合否と記録**：

- 合否に置く行は、IP-1、IP-3、IP-5、IP-6、IP-8 の全行と、IP-2 の T-IP-02-001（`lat-dialog` の run が 1 本、欠けなく記録されること。`stream.machine.refused` なし。`final` を持つ標本の半分以上が `translation` を持つこと）である。
- 道具の行（数える式、join の式、page の dump を読む式が、既知の入力を正しく数えるかを測る行）は、合成の入力で走り、IP-2、IP-4、IP-7 の道具の行も合否に置く。道具が正しいことは、実走の数がどうであれ崩さない。
- 実走の数（取り違えた発話の数、時間の差）は記録だけである。IP-2 と IP-7 の質の線と IP-4 の時間の線は「記録だけ。完成の合否に置かない（評議 2026-10-22）」。線を超えた時は、超えたと記録の file に書き、test は落とさない。
- 仮の値の行は IP-2、IP-4、IP-7 の線である。行に「仮の値。10.9.1 の置き直しの後に書き直す」と書く。

**材料**：

- IP-1 の行は `hayamimi-relay/tests/support/events.rs` の `FakeRecognizer`、`FakeLlm`（遅れ 0）、`run_relay`（`Lanes::PerSource`）、`translations_of`、`check_delivery` を使う。`Sent` に `source` が `file` の行を作る口（`Sent::file(seq, of, lang)`、id は `file-<seq>`）を足す。訳の期待値は `Sent::translated` の文字置換（`english_of` と `japanese_of`）で、中継の code を通らない。
- IP-6 の fixture は、値が一定の wav（標本が全部 3000）である。長さが 320 標本（1 枠）未満なら `speech_end` は clip の長さそのものを返す（`manifest.rs:74`。前例は `a_join_gives_each_clip_its_span_and_no_trailing_silence`）。ja の clip は `a1` 100、`a2` 60、`a3` 80 標本、en の clip は `b1` 50、`b2` 70、`b3` 90 標本。間 10 ms は 160 標本（16 kHz）。pinned manifest 1 本に ja、ja、ja、en、en、en の順で 6 行を置き、`--part manifest:<m>:ja` と `--part manifest:<m>:en` の 2 つで取る（10.7 の command と同じ形）。
- IP-2、IP-4、IP-7 の道具の行の fixture は、test が `env!("CARGO_TARGET_TMPDIR")` の下に作る合成の記録である（前例は T-RP-61-004）。合成の記録は、発話 4 つ（manifest の `lang` が ja、en、ja、en）の 3 回の繰り返しで、final は `latency::build_samples` と同じ割り当て（その発話が始まった後に着いた final は、始まった最新の発話の物）で置く。
- IP-3 の偽は、前例の `native/hayamimi-asr/tests/lid_dual.rs` の `Named`（`Identify`）と `Models`（`Decode`）の形である。偽の判定器は答えの列を持ち、呼ぶたびに次を返す。偽の `Decode` は呼ばれた tier を順に覚える。偽の SenseVoice の印は、表の中では code（ja、en、zh、ko、yue）だけを書く。test の中では `lid_dual.rs` の `GOLDEN` の `sense_voice` の列と同じ raw tag（code を山括弧と縦棒で包んだ形）にする。
- IP-3 の `step` と `lid` は、`Router::transcribe` が返す `Transcript` の `fallback` から読む（設計 10.4。落ちた時だけ値を持つ。`None` は落ちていない）。`step` の値は `sv_tag`、`script`、`default`、`sv_failed` の 4 つの名前で、`lid` は whisper-tiny の答え（`zh` など）か、空の文字列、`Err`、判定の段が無い時の答え無し（表の中では `null`）である。型の名前と読み口の形は IP-S3 が決め、表の中の「`step` が X」「`lid` が Y」は、その 2 項目の値を指す。
- IP-8 の行は `native/hayamimi-eval/tests/support/s83.rs` の `ProcRoot`（`--proc-root` の fixture）、`FakeIngest`、`Setup`、RP-83 の S83o と同じ stat の値（utime と stime の和の合計 3751）を使う。fixture は pid 4242（comm が `hayamimi`）、4243（`hayamimi-relay`）、4244（`llama-server`）の 3 つで、4244 の `cmdline` は NUL 区切りの語と、最後の語の後ろの NUL である。表の中の「cmdline が A、B、C」はその語の並びを指す。`stream` は `Setup` の標準の引数（`--proc-root`、`--wav`、`--manifest`、`--out` ほか）に、各行が言う `--own-pid` と `--own-llama-port` を足して起こす。`own_args` が `--own-llama-port` の値を引数で受ける形に直すのは IP-S1 の席である（設計 10.11 の IP-S1）。
- IP-5 の偽の訳の server は `fake_llama_server -m translates.gguf --port <llm port>`（`hayamimi/src/bin/fake_llama_server.rs` に IP-S2 が足す。設計 10.11 の 4）である。丸ごとの答えと stream の答えの両方を返し、固定の文は V に止められることがある（設計 10.12 の H40）。IP-5 は訳の `status` が `ok` でも `held` でも 1 つと数え、文の照らしは `ok` の訳だけに行う。
- IP-3 の `route/tests/pair.rs` は `Script`、`FixedLid`、`router` の補助（`route/tests/mod.rs`）を使い、`session.pending` と `last_lang()` を直に読む。
- IP-3 の `pipeline.rs` の行（036 から 039）は、`pipeline.rs` の中の型 `Solved` と `endpoint::State<Solved>`（`hold`、`plan_with_gap`。前例は `endpoint.rs` の `a_kept_sentence_is_used_when_the_segment_ends_within_a_window_of_its_range`）を直に使い、router も model も port も使わない。`Solved` の `fallback` 以外の項目は 0 と空の文字列で埋める。`fallback` の項目の名前と、`Solved` から `Final` を作る関数の名前は IP-S3 が決める（設計 10.10）。落ちの印は、(a) `step` が `sv_tag`、`lid` が `zh`、(b) `step` が `sv_failed`、`lid` が答え無し、の 2 つを使う。
- `lat-short` の作り方は、IP-S1 の実装の席が行う。手元の日本語と英語の声（CS-1 と CS-2 と同じ声）で、1 秒未満の返事を言語ごとに 10 本、16 kHz、mono、16 bit の wav にして `testdata/lat-short/` に置き、`wav`、`lang`、`ref` を持つ manifest を書き、`fetch-set --pin` で固め、`fetch-set --concat --interleave --part manifest:<pinned>:ja --part manifest:<pinned>:en --gap-ms 800 --out testdata/manifests/lat-short.json --wav-out testdata/lat-short.wav` でつなぐ（`--interleave` は IP-S1 で足す）。作った物は T-IP-07-002 から 007 が測る。
- IP-S0 の試走（2026-10-22。転記：試走の席の申告で、この席は測っていない）は、自作の台本と合成音声の 1 本で、`ja→en` と `en→ja` の訳が両方出て、final 61 件のうち取り違え 0、`translate.line.skipped` 0 だった。ja の発話が読点で 2 つの final に割れ、閉じる final の訳が後半だけになった例がある。母数が小さいので、IP-2 の線の根拠にしない。割れた発話は T-IP-02-012 の入力の形である。

**数え方**：

- **取り違えた発話**：repeat ごとに、その発話に割り当てられた final のどれか 1 つでも `lang` が manifest の `lang` と違えば、その repeat は「取り違え」。3 repeat のうち 2 以上が「取り違え」の発話を「取り違えた発話」とする。母数は manifest の発話の数で、繰り返しを母数に足さない（設計 10.9.2 の 1 から 4）。
- **割れた発話**：1 つの発話が 2 つ以上の final に割れても、全部が manifest の `lang` なら取り違えではなく、`finals` が 2 以上の発話として別に数える（設計 10.9.2 の 2）。
- **final なし**：final が 1 つも無い repeat は、取り違えにも正しいにも数えず、「final なし」として別に数える。3 repeat とも final の無い発話は、判定なしの発話として一覧に名指す（設計 10.9.2 の 3）。
- **向き**：manifest が ja で final が en なら `ja_as_en`、manifest が en で final が ja なら `en_as_ja`、final の `lang` が ja でも en でもなければ `other`（設計 10.9.2 の 5）。
- **nearest-rank**：n 個の p% は小さい方から `(p × n + 99) / 100`（整数の割り算）番目（T 4.1）。
- **1 秒未満**：`end_sample − start_sample` が 16000 標本未満。16000 標本ちょうどは 1 秒未満に入らない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-IP-01-001 | 受入：`FakeRecognizer` と `run_relay` に、`source` が `file`、`type` が `final`、id が `file-1` から `file-10`、`lang` が ja、en の交互（`file-1` が ja）の 10 行を、300 ms の間で送る。text は `japanese_text(n)` と `english_text(n)` | `lang` が ja の行と en の行。切り替え 9 回 | 10 行の id のすべてに `settled: true` の translation がちょうど 1 件 | `translations_of` が 10 件。ja の 5 行は `src_lang` が `ja`、`lang` が `en`、`text` が `english_of(原文)`。en の 5 行は `src_lang` が `en`、`lang` が `ja`、`text` が `japanese_of(原文)`。全件の `source` が `file`、`status` が `ok`、`src_sha256` が原文の sha256。`translate.line.skipped` が 0 件（specified、IP-1。text は pseudo：文字置換で、中継の code を通らない。`check_delivery` と同じ式） | 陽性 | `ip01_one_source_is_translated_both_ways_by_line_language` |
| T-IP-01-002 | 001 と同じ 10 行を、間を空けずに続けて送る | 間の下端（0 ms）。1 つの入力の行は中継の lane 1 本に並ぶ（設計 H22） | 001 と同じ | 001 と同じ 10 件で、取りこぼし 0、重複 0（specified：IP-1 の述語を、間を詰めた入力で打つ） | 境界、陽性 | `ten_alternating_file_lines_sent_back_to_back_each_get_one_settled_translation` |
| T-IP-01-003 | 001 と同じ 10 行を `source` が `mic`、id が `mic-1` から `mic-10` で送る | 無効類：mic の en の行 | mic は ja だけを訳す | translation が 5 件（奇数番の ja の行、`lang` が `en`）。偶数番の en の 5 行は translation が 0 件で、`translate.line.skipped` が 5 件、全件の `reason` が `language`（specified、IP-1 の陰性対照。`line.rs` の `Source::translates`） | 陰性 | `a_microphone_line_in_english_is_noted_and_not_translated` |
| T-IP-01-004 | 001 と同じ 10 行を `source` が `loopback`、id が `loopback-1` から `loopback-10` で送る | 無効類：loopback の ja の行 | loopback は en だけを訳す | translation が 5 件（偶数番の en の行、`lang` が `ja`）。奇数番の ja の 5 行は `translate.line.skipped` が 5 件、`reason` が `language`（specified、同上） | 陰性 | `a_loopback_line_in_japanese_is_noted_and_not_translated` |
| T-IP-01-005 | `source` が `file`、`lang` が `zh`、id が `file-1` の 1 行、続けて `lang` が `ja`、id が `file-2` の 1 行 | 無効類：ja、en の外の言語（設計 10.3 の足りない所の 1） | ja、en の外は訳さず、次の行は訳す | `file-1` の translation が 0 件で `translate.line.skipped` が 1 件（`reason` が `language`）。`file-2` は `settled: true` の translation が 1 件（`lang` が `en`）。lane は止まらない（specified：設計 10.3。前例は `r8a_a_line_that_is_not_translated_is_only_noted`）。001 の「skipped が 0 件」を数える式が、skipped の在る入力で 1 を数える | 陽性、境界 | `a_file_line_in_a_third_language_is_noted_and_the_next_line_is_translated` |
| T-IP-02-001 | 実走：設計 10.9 の IP-2 の command で、`hayamimi --input ws --mode pair --source file --serve 8833`、`hayamimi-relay live --events http://127.0.0.1:8833/events --llm http://127.0.0.1:<訳の server の port> --serve 8835`、`stream --wav testdata/lat-dialog.wav --repeat 3 --relay-port 8835 --own-pid <hayamimi の pid> --own-pid <中継の pid> --own-pid <訳の server の pid> --own-llama-port <訳の server の port> --out ip02_pair.jsonl` を流す。続けて `hayamimi` を `--mode fast` で起こし直し、同じ形（`--own-pid` 3 つと `--own-llama-port` を含む）で `--out ip02_fast.jsonl` の run も取る。どの `stream` も、始める前に machine が静かになるのを待つ（`wait_for_quiet_machine`。load が下がらなければ `Err`）。`hayamimi` と中継の stdout と stderr は、走っている間 別の thread が読み続ける（T-IP-02-022） | `lat-dialog` の 30 発話 × 3 repeat。pair の run 1 本と fast の run 1 本 | 記録が欠けない（合否）。取り違えの数は記録だけ | 合否：`ip02_pair.jsonl` が 90 行（repeat 1 から 3 と utt 1 から 30 が 1 回ずつ）、`.events.jsonl`、`.summary.json`、`.machine.jsonl` が在り、`stream.machine.refused` が無く、final が 1 件以上あり、final を持つ標本の数の半分以上の標本が translation を持つ（`check_translated`。T-IP-02-023）（specified：IP-2 の「run を 1 本記録する」と、設計 10.9 の「子の出力」。評議 2026-10-22）。記録：取り違えた発話の数を向きごとに、`asr.lang.fallback` の数を `step`（4 つ）ごとに、`asr.lang.fallback` の `id` と final の突き合わせの結果（T-IP-02-016、017 の式。`Err` は IP-3 の落ちとして調べ、test は落とさない）、`lang` が ja でも en でもない final の数、原文と同じ言語の translation の数、`finals` が 2 以上の発話の数、final なしの repeat の数を、fast の run も同じ式で、記録の file に書く。取り違えた発話が 2 を超えても落とさない。記録だけ。完成の合否に置かない（評議 2026-10-22）。仮の値。10.9.1 の置き直しの後に書き直す（線は 2） | 陽性 | `ip02_each_dialog_utterance_is_read_in_its_own_language` |
| T-IP-02-002 | 合成の記録：3 repeat × 4 発話（manifest が ja、en、ja、en）の 12 標本の jsonl | 完全な記録 | repeat 3 本 × 発話の数 | 記録の完全さを見る関数が `Ok` で、標本の数 12 を返す（specified：欠けない記録。90 行の式を 3 × 4 で打つ） | 陽性 | `a_record_of_three_repeats_of_every_utterance_is_complete` |
| T-IP-02-003 | 002 の jsonl から repeat 3 の utt 4 の 1 行を抜いた物。別に、0 行の jsonl | 欠けた 1 行（境界）。0 行（空） | 全部が 1 回ずつ | 1 行抜いた物は `Err` で `repeat 3` と `utt 4` を名指す。0 行は `Err`（0 件を成果と読まない。implicit。002 が陽性対照） | 境界、陰性 | `a_record_with_a_missing_line_or_no_line_is_refused` |
| T-IP-02-004 | 合成の記録：発話 2（manifest が en）の final の `lang` が repeat 1 から 3 で en、ja、ja。ほかの発話は全部 manifest どおり | 3 repeat のうち 2 が違う（線の上） | 3 のうち 2 以上で違えば取り違え | 取り違えた発話が 1 つ（発話 2）で、向きは `en_as_ja` が 1、`ja_as_en` が 0（specified：10.9 の IP-2 の数え方。手で数えた） | 陽性、境界 | `an_utterance_read_wrongly_in_two_of_three_repeats_is_mixed_up` |
| T-IP-02-005 | 合成の記録：発話 3（manifest が ja）の final の `lang` が repeat 1 から 3 で en、ja、ja。ほかは全部 manifest どおり | 3 repeat のうち 1 が違う（線の下 1） | 同上 | 取り違えた発話が 0（specified、同上。004 が対の陽性） | 境界、陰性 | `an_utterance_read_wrongly_in_one_of_three_repeats_is_not_mixed_up` |
| T-IP-02-006 | 合成の記録：全 final の `lang` が manifest どおり | 取り違えが無い | 取り違えが 0 | 取り違えた発話が 0、`other` が 0（specified。004 が数える式が 0 でない値を数える陽性対照） | 陰性 | `a_run_where_every_final_has_the_utterances_language_has_no_mixed_up_utterance` |
| T-IP-02-007 | 合成の記録：30 発話（manifest が ja と en の交互）の全 final の `lang` が `ja` | `--lang ja` の run の形（設計 10.9 の IP-2 の陰性対照） | en の発話が全部取り違え | 取り違えた発話が 15、向きは `en_as_ja` が 15、`ja_as_en` が 0。15 は線の 2 を超える（specified：10.9 の IP-2 の陰性） | 陰性 | `a_run_forced_to_ja_mixes_up_every_english_utterance` |
| T-IP-02-008 | 取り違えた発話の数 2 と 3 を、線の比べの関数に渡す | 線の境界（仮の値 2）の両側 | 数 ≤ 2 | 2 は「線の内」、3 は「線の外」を返し、どちらも `Ok`（比べの結果を記録に書く関数で、落とさない）。記録の file に「線の内」「線の外」と線の値 2 が書かれる（specified：評議 2026-10-22。仮の値。10.9.1 の置き直しの後に書き直す） | 境界 | `two_mixed_up_utterances_are_on_the_line_and_three_are_over_it` |
| T-IP-02-009 | 合成の記録：final の `lang` が `zh` の標本を 1 つ含む記録。別に、全部 ja か en の記録 | ja、en の外の final | 0 が期待 | 含む記録は `other` の final が 1、含まない記録は 0（specified：10.9 の IP-2。006 が 0 の陽性対照）。1 以上は IP-3 の落ちとして調べる | 陽性、陰性 | `a_final_in_a_third_language_is_counted` |
| T-IP-02-010 | 合成の記録：`src_lang` と `lang` が同じ translation（`src_lang` が `ja`、`lang` が `ja`）を 1 つ含む events。別に、全部 `src_lang` と `lang` が違う events | 原文と同じ言語の translation | 0 が期待 | 含む events は 1、含まない events は 0（specified：10.9 の IP-2） | 陽性、陰性 | `a_translation_into_its_own_source_language_is_counted` |
| T-IP-02-011 | 合成の log：`asr.lang.fallback` の行が 4 つ（`step` が `sv_tag`、`sv_tag`、`default`、`sv_failed`。`sv_failed` の行の level は `warn`、ほかは `info`）。別に、その行が 0 の log。別に、log の file が無い run | `step` ごとの数（4 つの値）。0 件の log。log が無い | 数を記録する | 4 行の log は `sv_tag` が 2、`script` が 0、`default` が 1、`sv_failed` が 1。0 行の log は 4 つとも 0。file が無い run は `Err` で path を名指し、0 と読まない。`step` が 4 つの外の行は `Err` で値を名指す（specified：設計 10.4 の step の 4 つの値。implicit：読めない log を 0 と読まない。1 つめが 0 でない陽性対照） | 陽性、境界、陰性 | `language_fallback_events_are_counted_by_step_and_a_missing_log_is_an_error` |
| T-IP-02-012 | 合成の記録：発話 1（manifest が ja）に final が 2 つ割り当てられ、どちらも `lang` が `ja`（IP-S0 の試走で見た、読点で割れる形）。別に、2 つのうち 2 つめだけ `en` | 1 発話に final が複数 | 数え方の「どれか 1 つでも違えば取り違え」 | 前者は `finals` が 2 の発話として数えられ、取り違えではない。後者はその repeat が取り違え（3 repeat とも同じなら取り違えた発話 1）（specified：設計 10.9.2 の 2「1 つの発話が 2 つ以上の `final` に割れても、全部が manifest の `lang` なら取り違えではなく、`finals` が 2 以上の発話として別に数える」。割れの分だけ正しいと数える build は、後者を取り違えと数えず落ちる） | 境界 | `an_utterance_split_into_two_finals_is_counted_once_and_a_wrong_one_among_them_makes_it_wrong` |
| T-IP-02-013 | 合成の記録：発話 4 の repeat 2 に final が無く、repeat 1 と 3 は manifest どおり | final の無い repeat | 取り違えにも正しいにも数えない | 発話 4 の取り違えは 0 で、「final なし」が 1。3 repeat とも final が無い発話は、判定なしの一覧に載る（specified：設計 10.9.2 の 3。final の無い repeat は取り違えにも正しいにも数えず、「final なし」として別に数え、3 repeat とも final の無い発話は、判定なしの発話として一覧に名指す。001 の記録の「final なしの repeat の数」の元。final の無い repeat を正しいと数える build は、「final なし」が 0 になって落ちる） | 境界 | `a_repeat_with_no_final_is_not_counted_as_right_or_wrong` |
| T-IP-02-014 | 合成の manifest：30 行、`lang` が ja、en、ja、en…の交互 | 交互の manifest の切り替え | 切り替えが 29 回 | 切り替えを数える関数が 29 を返す。同じ関数に ja が 15 行続いてから en が 15 行続く manifest を渡すと 1（specified：10.9 の IP-2 の陽性対照の式。交互でない物を 1 と数える陰性） | 陽性、陰性 | `a_manifest_that_alternates_has_one_switch_fewer_than_utterances` |
| T-IP-02-015 | 実物：`testdata/manifests/lat-dialog.json` | 実物の manifest | 行 30、切り替え 29 | 行が 30、`lang` が ja 15 と en 15、切り替えが 29（specified：10.9 の IP-2 の陽性対照。014 と同じ関数） | 陽性 | `lat_dialog_has_thirty_utterances_and_twenty_nine_switches` |
| T-IP-02-016 | 道具：合成の log に `asr.lang.fallback` の行が 2 つ（`id` が 3 と 5）。合成の events に `type` が `final` の行が 6 つ（id が `file-1` から `file-6`。`--repeat 3` の run で 1 つの process が 1 から連番を振った形で、repeat 1 が `file-1` と `file-2`、repeat 2 が `file-3` と `file-4`、repeat 3 が `file-5` と `file-6`） | fallback の行の `id` と final の突き合わせ。有効類：どの行の `id` にも final が 1 つ在り、同じ `id` の行が重ならない。`id` が repeat をまたいで続く | 1 つの final に fallback の event は 1 件まで。final の無い `id` の行は無い。突き合わせは repeat を区別せず `id` だけで結ぶ | 突き合わせる関数が `Ok` で、結べた行の数 2 を返す。log の `id` n は final の id `file-n` に結ぶ（`id` 3 は repeat 2 の `file-3`、`id` 5 は repeat 3 の `file-5`）（specified：設計 10.4「`stream --repeat` の repeat は同じ process に流すので、repeat をまたいで `id` は続く」「突き合わせは、repeat を区別せずに、同じ process の log と events の中で `id` だけで結ぶ」と、「解いて持っただけで `final` にならなかった読みは event を出さず、`final` を出す所で、その `final` の `id` を付けた event を 1 件出す」。候補ごとに event を出す build を、IP-2 の run の記録で見つける道具。repeat ごとに `id` が 1 から振り直されると読み、`id` 3 を repeat 1 の `final` の中で探す build は、final が無いと読んで `Err` を返して落ちる。pseudo：手で数えた） | 陽性 | `a_fallback_event_is_matched_to_its_final_by_id_across_repeats` |
| T-IP-02-017 | 道具：3 入力。(a) `id` が 3 の `asr.lang.fallback` の行が 2 つ（持った候補の分と確定の分）で、events に `file-3` の final が 1 つ。(b) `id` が 9 の行が 1 つで、events に `file-9` の final が無い（持っただけで final にならなかった候補）。(c) `asr.lang.fallback` の行が 0 の log と、final が 6 つの events（(b) と (c) の events は 016 と同じ `file-1` から `file-6`） | 1 つの final に event が 2 件。final の無い `id` の event。event が 0 件 | 1 つの final に fallback の event は 1 件まで。final の無い `id` の行は無い | (a) は `Err` で `id` 3 と 2 件を名指す。(b) は `Err` で `id` 9 と、final が無いことを名指す。(c) は `Ok` で結べた行の数 0（specified：設計 10.4。016 が 2 を返す陽性対照。(a)(b) を通す build は、候補ごとに event を出す code を IP-2 の run で見逃す。ここは道具の行で、実走の記録は 001 が書く） | 陰性、境界 | `a_fallback_event_twice_for_one_final_or_for_no_final_is_an_error` |
| T-IP-02-018 | 道具：2 つの run の記録を混ぜた入力。(a) events は pair の run の final（`file-1` から `file-6`）に、fast の run の final（`file-1` から `file-6`）を続けてつないだ 12 行。log は `id` が 3 の行 1 つ。(b) 016 と同じ log と events（1 つの run の物） | 別の process の `id` は、どちらも 1 から振られて重なる。同じ run の記録だけ | 1 つの `id` に final は 1 つ。2 つの run の記録を混ぜて結ばない | (a) は `Err` で、`file-3` の final が 2 つ在ることを名指し、結べた行の数を返さない。(b) は `Ok` で 2（specified：設計 10.4「`--mode pair` の run と `--mode fast` の run は別の process で、`id` はそれぞれ 1 から振られるので、2 つの run の記録を混ぜて結ばない」。2 つの run の `id` は重なるので、混ぜた events では同じ `id` の final が 2 つになる。(a) で最初の final だけに結ぶ build は `Ok` を返して落ちる。016 と (b) が混ぜない入力の陽性対照。この式が当てるのは同じ `id` の final が重なる混ぜ方だけで、log を別の run から取り、events の `id` が重ならない組は当たらない：001 は同じ `--out` の組の log と events だけを渡す） | 陰性、陽性 | `records_of_two_runs_mixed_together_are_refused_and_one_run_is_matched` |
| T-IP-02-019 | 道具：test が組む proc root（`<pid>/comm` と NUL 区切りの `<pid>/cmdline`）。(a) `comm` が `llama-server` の pid が 2 つ。cmdline は `llama-server`、`-m`、`model.gguf`、`--port`、`18081` の pid と、同じ形で最後の語が `18084` の pid。(b) (a) から 18081 の pid を除いた root | 他の port の server が居る時と居ない時。選ぶ port は 18084 | 集まる pid はちょうど 1 つ | (a) も (b) も `find_llama(<root>, 18084)` が `Ok` で、pid が 18084 の pid、`port` が 18084。(a) の結果は (b) と同じ（specified：設計 10.7.2 の 1、2。18081 の server が起きている機械でも、run の server を数でなく port で選ぶ。server を数えて選ぶ形は、(a) で 2 つと数えて `Err` を返す） | 陽性 | `the_run_picks_the_llama_server_on_its_own_port_beside_another_one` |
| T-IP-02-020 | 道具：019 と同じ組み方の 4 つの root。(a) 18081 の pid だけ。(b) cmdline の `--port` の次の語が `018084` の pid だけ。(c) cmdline が `--port`、`8080`、`--port`、`18084` の pid だけ（`--port` が 2 つ）。(d) 18084 の pid が 2 つ | 0 件、桁の違う綴り、`--port` が 2 つ、2 件 | ちょうど 1 つの時だけ `Ok` | 4 つとも `find_llama(<root>, 18084)` が `Err`。(a)(b)(c) の `Err` は `18084` と `hayamimi-llama-e2e.service` を名指し、(d) の `Err` は 2 つの pid を名指す。019 の (b) が `Ok` を返す（specified：設計 10.7.2 の 2、3。`stream --own-pid` の門と同じ読み方で、`018084` と `--port` が 2 つの server は 18084 の物に数えない。`018084` を 18084 と読み、最初の `--port` だけを読む形は、(b) と (c) で `Ok` を返して、門が拒む server を選ぶ） | 陰性、境界 | `a_run_without_exactly_one_llama_server_on_its_own_port_is_refused` |
| T-IP-02-021 | 道具：proc root に pid が 4 つ。`comm` が `llama-server` で cmdline が `--port 18084` の pid、`comm` が `hayamimi` で cmdline に `--port 18084` を持つ pid、`comm` が `llama-server` で cmdline の無い pid、`comm` が `llama-server` で `--port 18081` の pid。別に、存在しない proc root | 名前と port の両方が合う物、名前だけ合う物、port だけ合う物、cmdline が読めない物。読めない root | 返る pid は名前と port の両方が合う物だけ | `llama_server_pids(<root>, 18084)` が `Ok` で、返る pid は 1 つ目の pid だけ。存在しない root は `Err`（specified：設計 10.7.2 の 1、3。cmdline が読めない process は数えずに飛ばし、root が読めない時だけ `Err`） | 陽性、陰性 | `only_a_llama_server_naming_the_port_is_listed` |
| T-IP-02-022 | 道具：`sh` の子を 2 つ起こす。どちらも stdout と stderr に 400 万 byte ずつ書いて終わる。1 つ目は `drain_output` を呼び、2 つ目は呼ばない（unix だけ） | 子の出力が pipe の容量を超える形。読む者が居る子と居ない子 | 400 万 byte は、この機械の WSL2 の kernel の pipe（1 MiB）の約 3.8 倍、Linux の既定（64 KiB）の約 61 倍 | 1 つ目は 20 秒以内に終わる。2 つ目は 2 秒たっても終わらない（test は 2 つ目を kill して wait する）（specified：設計 10.9 の「子の出力」。読まれない pipe は容量で書き手を止める。2 つ目が終わったら、test は pipe が満ちたことを見られないので落ちる） | 陽性、陰性 | `a_child_whose_output_is_drained_never_stops_on_a_full_pipe` |
| T-IP-02-023 | 合成の samples file：`final` を持つ標本が 78 件で、`translation` を持つ標本が 78、39、38、15 件の 4 つ。`final` を持つ標本が 0 件の file。JSON でない行 | 訳が揃った run、訳が半分の run、半分に 1 足りない run、訳が止まった run、`final` の無い run、読めない行 | `translation` を持つ標本の数の 2 倍と、`final` を持つ標本の数の比べ（39 は通り、38 は落ちる） | 78 と 78 は `Ok((78, 78))`、78 と 39 は `Ok((78, 39))`。78 と 38 は `Err`。78 と 15 は `Err` で `15` と `78` を名指す。`final` の無い file は `Err`（`no sample has a final`）。JSON でない行は `Err`（specified：設計 10.9 の「子の出力」と IP-2 の合否の行。0 件を成果と読まない。78 と 78 が陽性対照） | 陽性、境界、陰性 | `a_run_whose_translations_stopped_is_refused` |
| T-IP-03-001 | 受入：`Router::new(偽の Decode, Some(偽の判定器), Policy::pair())`。偽の判定器の答えは `zh`、`ko`、空の文字列、`Err` の 4 つ。偽の SenseVoice（`Tier::Sv`）の答えは、(a) 印 en、文 `yes`、(b) 印 zh、文 `thank you`、(c) 印 zh、文 `ありがとう`、(d) 印 zh、文が空の 4 つ。4 × 4 の 16 の組を打つ（偽の Decode は前例 `lid_dual.rs` の `Models` と同じ形で、`Tier::Sv` 以外の tier は空でない文を返す。偽の判定器の `Err` は `AsrError` の任意の 1 つ） | 判定器の答えが ja、en の外（zh、ko）、答え無し（空）、失敗（`Err`）。SenseVoice の答えが印 en、印が外で文がラテン文字、印が外で文が仮名、文が無い | 選ばれる言語が ja か en | (a) は 4 つとも `Transcript.lang` が `en`、`Transcript.fallback` の `step` が `sv_tag`。(b) は 4 つとも `en`、`step` が `script`。(c) は 4 つとも `ja`、`step` が `script`。(d) は 4 つとも `ja`、`step` が `default`。`lid` は、判定器が `zh` の組で `zh`、`ko` の組で `ko`、空の文字列の組と `Err` の組で答え無し。16 の組すべてで `Tier::Sv` の呼びが 1 回、`fallback` が `Some`、router は `Err` を返さない（specified：設計 10.4 の 1 から 4 と 10.9 の IP-3。`step` と `lid` の読み口は設計 10.4 の `Transcript` の `fallback`） | 陽性、境界 | `ip03_an_unsure_identifier_falls_to_ja_or_en` |
| T-IP-03-002 | 001 の (a) から (d) を、判定の段が無い router（`Router::new(偽の Decode, None, Policy::pair())`）で打つ | 判定の段が無い（`lid` が `None`） | 001 と同じ | 4 つとも 001 と同じ `lang` と `step`。`lid` は 4 つとも答え無し（specified：10.4 の 2 の「判定の段が無い」と `lid` の「判定の段が無い時は `null`」、10.9 の IP-3 の「判定の段が無い router でも同じ」） | 境界 | `a_router_with_no_identifier_falls_to_ja_or_en` |
| T-IP-03-003 | 偽の判定器が `ja` を返す segment と、`en` を返す segment を、`Policy::pair()` で 1 つずつ。偽の Decode は `Tier::Sv` の呼びを数える | 判定が ja、en（有効類） | 判定が ja か en なら SenseVoice を呼ばない | どちらも選ばれる言語が判定器の答えと同じ（`ja`、`en`）。`Tier::Sv` の呼びが 0 回、`Transcript.fallback` が `None`（specified：10.4 の 1 と 10.9 の IP-3 の陽性対照「`fallback` は無い」。001 が `fallback` を `Some` と読む式の対照にもなる） | 陽性 | `an_identifier_that_says_ja_or_en_never_asks_sensevoice` |
| T-IP-03-004 | 001 の (a) と同じ偽（判定器が `zh`）を `Policy::fast()` で | 対照：pair でない mode | fast は ja、en に絞らない | 選ばれる言語が `zh`（specified：10.9 の IP-3 の陰性対照）。同じ偽を `Policy::pair()` に渡すと `en`（001 の (a)） | 陰性 | `fast_mode_follows_an_identifier_that_says_zh` |
| T-IP-03-005 | 偽の判定器は答えの列（`ja`、`en`）を持ち、呼ぶたびに次を返す。偽の SenseVoice は常に印 ja。`speech_s` は 3.0 秒の live の segment を 2 つ読む。`Policy::balanced()` と `Policy::pair()` で、それぞれ別の router | balanced は SenseVoice と一致するまで直前の言語に留める。pair は判定に従う | 設計 10.3 | balanced は 2 つめも `ja`、pair は `en`（specified：10.3「切り替えのたびに SenseVoice の読みとの一致を求め、合わなければ直前の言語に留める」。pair が balanced と違うことを見る） | 陰性 | `balanced_mode_holds_the_session_language_against_a_sensevoice_that_disagrees` |
| T-IP-03-006 | 判定器が `zh`、偽の SenseVoice の印が zh、文が `日本語だabcdef`（文字 10、仮名と漢字 4） | 仮名と漢字が文字の 3 割を超える（4/10。境界の上側）。3 割ちょうどは 029 が持つ | 3 割を超えれば ja | `lang` が `ja`、`step` が `script`（specified：10.4 の 3。文字の数え方は `lid.rs` の `count_scripts` と同じ：`is_alphabetic` の文字を数える） | 境界 | `a_text_with_more_than_three_tenths_kana_and_kanji_is_ja` |
| T-IP-03-007 | 判定器が `zh`、偽の SenseVoice の印が zh、文が `Hello there` | ラテン文字だけの文 | ラテン文字だけなら en | `lang` が `en`、`step` が `script`（specified：10.4 の 3）。006 と合わせて、文字の 2 つの規則が別の答えを出す | 陽性 | `a_text_of_latin_letters_only_is_en` |
| T-IP-03-008 | 判定器が `zh`、偽の SenseVoice の印が zh、文が空の文字列 | 文が無い（空） | 文字が 1 つも無ければ ja | `lang` が `ja`、`step` が `default`（specified：10.4 の 4） | 境界 | `a_sensevoice_reading_that_is_empty_falls_to_ja` |
| T-IP-03-009 | 判定器が `zh`、偽の SenseVoice の印が zh、文が `。、！` | 文があるが文字（`is_alphabetic`）が 0 | 文字が 1 つも無ければ ja | `lang` が `ja`、`step` が `default`（specified：10.4 の 4「記号だけ」） | 境界 | `a_sensevoice_reading_of_symbols_only_falls_to_ja` |
| T-IP-03-010 | 判定器が `zh`。偽の Decode の `Tier::Sv` が `DecodeError::Unavailable` を返す | SenseVoice の model が無い | SenseVoice も無ければ ja | `lang` が `ja`、`step` が `default`、router は `Err` を返さない（specified：10.4 の 4「SenseVoice の model が無い（`DecodeError::Unavailable`）：`ja`」。model が無ければ読む文が無いので 3 を飛ばして 4 に入る：10.4 の 3 の後の段落。SenseVoice の復号が失敗した時は 025 が持つ） | 境界 | `an_unavailable_sensevoice_falls_to_ja` |
| T-IP-03-011 | 判定器が `ko`、偽の SenseVoice の印が ko、文が `thank you very much` | SenseVoice の印が ko（ja、en の外。zh は 001 が持つ） | 印が外なら文の文字で決める | `lang` が `en`、`step` が `script`（specified：10.4 の 3） | 境界 | `a_sensevoice_tag_of_ko_is_outside_the_pair_and_the_text_decides` |
| T-IP-03-012 | 判定器が `zh`、偽の SenseVoice の印が yue、文が `ありがとう` | SenseVoice の印が yue | 印が外なら文の文字で決める | `lang` が `ja`、`step` が `script`（specified：10.4 の 3。`sv_lid_tag` が yue を返す印） | 境界 | `a_sensevoice_tag_of_yue_is_outside_the_pair_and_the_text_decides` |
| T-IP-03-013 | 判定器が `ja`。ja の recognizer（`Tier::Rz`）が Hangul の文 `안녕하세요` を返す | 決めた後の読み直しが ja、en の外（ko）を示す | pair は ja、en の外へ動かさない | `lang` が `ja` のまま（`ko` にならない）。前提として、`lid::script_corrected_lang("ja", "안녕하세요")` を直に呼ぶと `ko` を返す（その戻りを pair は採らない）（specified：10.4「決めた後の読み直しも ja と en の外へは動かさない」。derived：関数を直に打った戻り） | 陰性 | `a_japanese_reading_in_hangul_stays_ja_in_pair_mode` |
| T-IP-03-014 | 判定器が `ja`。ja の recognizer が `thank you very much` を返し、en の tier が同じ文を返す | 読み直しが ja から en（pair の中）へ動く | pair の中の読み直しは働く | `lang` が `en`、文が en の tier の物。前提として、`lid::script_corrected_lang("ja", "thank you very much")` を直に呼ぶと `en` を返す（specified：10.4。013 が外へ動かさない対の陰性。pair の中の動きまで止めていないことを見る陽性。derived：関数を直に打った戻り） | 陽性 | `a_japanese_reading_in_latin_letters_is_read_again_as_en_in_pair_mode` |
| T-IP-03-015 | 偽の判定器は答えの列（`ja`、`en`、`ja`、`en`）を持ち、呼ぶたびに次を返す。偽の SenseVoice は常に印 ja。`speech_s` は 0.3 秒の live の segment を 4 つ、`Policy::pair()`。同じ入力を `Policy::balanced()` の router にも流す | 短い発話（0.3 秒）で言語が毎回変わる | pair は毎発話の判定に従う | pair の `lang` が ja、en、ja、en。balanced の 2 つ目は `ja` に留まる（`resolve_dual_confirm` は 0.5 秒未満で直前の言語を返す）（specified：10.4「session の言語に留めずに発話ごとに判定に従う」。balanced の 2 つ目だけを置き、3 つ目以降は置かない） | 陽性、陰性 | `pair_mode_follows_each_detection_even_when_the_utterance_is_short` |
| T-IP-03-017 | `pair.rs`：live の segment（判定器 `ja`）の後、`Request { live: false, .. }` の segment（判定器 `en`）を読む | 非 live の decode（清書の `read_known` の形）。`session.last_lang` と `session.pending` を読む | 非 live は session の言語を動かさない（設計 10.11 の IP-S3 の条件 3） | `last_lang()` が `Some("ja")` のまま、`session.pending` が `None` のまま（specified：条件 3「非 live の decode は今のとおり `last_lang` を書かない」。018 が live の decode は動かす陽性対照。候補は live の decode で解かれる（設計 10.4）ので、この行は清書の `read_known` の形だけを測る） | 陰性 | `a_decode_that_is_not_live_leaves_the_session_language_alone_in_pair_mode` |
| T-IP-03-018 | `pair.rs`：017 の最初の segment の後、live の segment（判定器 `en`）をもう 1 つ読む | live の decode が session の言語を動かす | live は動かす | `last_lang()` が `Some("en")`（017 が数える式が、動く入力で動きを見つける陽性対照） | 陽性 | `a_live_decode_in_pair_mode_sets_the_session_language` |
| T-IP-03-019 | `args.rs`：`--wav a --mode pair` を `parse` して `policy()` を呼ぶ | `--mode pair`（有効類） | pair の policy | `Policy::pair()` と等しい（specified：10.1「`--mode pair`」と 10.11 の IP-S3 の `mode_of` と `policy`） | 陽性 | `a_run_with_mode_pair_uses_the_pair_policy` |
| T-IP-03-020 | `args.rs`：`--wav a --mode pair --lang ja` を `policy()` に | `--lang` と `--mode pair` の同時指定 | `--lang` は判定を飛ばす（10.3） | `Policy::single("ja")` と等しい（specified：10.3「`--lang` を付けた run は判定を飛ばし」。IP-2 の陰性対照の `--lang ja` の run の形。`policy()` は `--lang` が mode に勝つ） | 陰性 | `a_lang_wins_over_mode_pair` |
| T-IP-03-021 | `args.rs`：`--wav a`（mode 無し）と `--wav a --mode fast` と `--wav a --mode single` を `policy()` に | 既存の mode と既定。`--mode pair` は flag の時だけ効く（評議 2026-10-22） | 既定を変えない | `Policy::balanced()`、`Policy::fast()`、`Policy::single("ja")`（specified：評議 2026-10-22「`--mode pair` は flag の時だけ効き既定を変えない」。既存の `a_run_identifies_languages_unless_told_to_read_one` と同じ式で、`L` の標本が変わらないことを見る） | 陽性 | `a_run_without_mode_keeps_the_balanced_default_and_fast_and_single_are_unchanged` |
| T-IP-03-022 | `args.rs`：`--wav a --mode paired`、`--wav a --mode Pair`、`--wav a --mode` | 無効類：綴りの違い、大文字、値無し | 4 つの名前の外は拒む | 最初の 2 つは `ArgError::ModeUnsupported`、3 つめは `ArgError::MissingValue`（specified：`mode_of` の既存の規則。前例は `a_mode_or_language_that_does_not_exist_is_refused`）。`--mode pair` は 019 で通る | 陰性 | `a_mode_name_outside_the_four_is_refused` |
| T-IP-03-023 | `args.rs`：`crate::strings::EN` と `crate::strings::JA` の text | 利用の案内の `--mode` の行と `mode_unsupported` の行（言語 2 つ × 2 行 = 4 行） | 4 行すべてが pair を名指す | 4 行すべてが `pair` を含み、かつ `balanced`、`fast`、`single` も含む（specified：10.11 の IP-S3「`strings.*.toml` の 4 行」。3 つの既存の名前が消えていないことが陽性対照） | 陽性 | `both_usage_texts_name_the_pair_mode_beside_the_three_old_ones` |
| T-IP-03-024 | `log.rs`：判定が落ちた発話の `asr.lang.fallback` の行を作る関数に、(a) `id` 7、`step` が `sv_tag`、`lid` が `zh` を渡す。(b) `lid` が答え無し（`None`）で `step` が `default`。(c) `step` が `sv_failed`、`lid` が `zh` | 項目は `id`、`step`、`lid`。`lid` が答え無し。失敗の `step` | 項目はこの 3 つだけ。level は `sv_failed` だけが `warn` | (a) と (b) は level が `info`、(c) は level が `warn`。3 つとも component が `asr`、項目が `id`（数）、`step`（`sv_tag`、`script`、`default`、`sv_failed` のどれか）、`lid`（文字列、答え無しは `null`）の 3 つだけ。`strings.en.toml` と `strings.ja.toml` の両方に `asr.lang.fallback` の説明が在る（specified：10.4。`/events` の wire は変えない：RP-12） | 陽性、境界 | `a_language_fallback_event_has_the_fields_the_design_names` |
| T-IP-03-025 | 3 入力。(a) 判定器が `zh`、偽の SenseVoice（`Tier::Sv`）が `DecodeError::Failed` を返す。(b) 判定器が `Err`、同じ失敗。(c) 判定の段が無い router（`None`）、同じ失敗。どれも `Policy::pair()`。ja の recognizer（`Tier::Rz`）は文 `はい` を返す | SenseVoice の復号が失敗した（`Unavailable` と別。設計 10.4 の 5）。判定器の答えが外、失敗、判定の段が無い | 失敗しても発話を止めない | 3 入力とも router が `Err` を返さず、`lang` が `ja`、`fallback` の `step` が `sv_failed`。`lid` は (a) が `zh`、(b) と (c) が答え無し。`Transcript.text` は `はい`（`Tier::Rz` の読みが返る）（specified：設計 10.4 の 5「SenseVoice の復号が失敗した（`DecodeError::Failed`）：`ja`」と「pair では、2 と 3 の SenseVoice の呼びだけが `Failed` を受け取って 5 に進み、発話は止まらない」。010 が `Unavailable`（`step` が `default`）の対。今の `try_decode` は `Failed` を `RouteError::Decode` にするので、pair の分岐が無い build は (a) から (c) で `Err` を返して落ちる） | 境界、陽性 | `a_failed_sensevoice_falls_to_ja_with_the_sv_failed_step` |
| T-IP-03-026 | 2 入力。(a) 判定器が `ja`、`Tier::Rz` が `DecodeError::Failed` を返す。(b) 判定器が `zh`、SenseVoice の印が en、文 `yes`、`en` の tier が `DecodeError::Failed` を返す。どれも `Policy::pair()` | SenseVoice の呼び以外の失敗（認識器の読みの失敗） | 飲むのは SenseVoice の判定の呼びだけ | 2 入力とも `Err(RouteError::Decode(..))`（`matches!` で見る）（specified：設計 10.4「2 と 3 の SenseVoice の呼びだけが `Failed` を受け取って 5 に進み」。ほかの tier の失敗は今のとおり呼び手へ返る。025 が SenseVoice の失敗を飲む対。全部の `Failed` を `ja` に落とす build は、ここで `Ok` を返して落ちる） | 陰性 | `a_failed_recognizer_other_than_the_sensevoice_probe_is_still_an_error_in_pair_mode` |
| T-IP-03-027 | 判定器は `zh` を 2 回返す。偽の SenseVoice の答えの列は、1 回目が `DecodeError::Failed`、2 回目が印 en、文 `yes`。`Policy::pair()` で live の segment を 2 つ読む。別に、1 回目を `DecodeError::Unavailable` にした同じ列を、別の router に打つ | 失敗の後の次の発話。`Failed` と `Unavailable` の違い | `Failed` は以後の呼びを止めない | 前者は 1 つ目が `ja`（`sv_failed`）、2 つ目が `en`（`sv_tag`）で、`Tier::Sv` の呼びが 2 回。後者は 2 つの segment とも `ja`（`default`）で、`Tier::Sv` の呼びが 1 回（specified：設計 10.4「`Failed` を `Unavailable` と同じに扱い、その tier を以後呼ばない形は採らない」。後者は今の `try_decode` が `Unavailable` の tier を以後呼ばない動き：derived） | 境界、陰性 | `a_failed_sensevoice_is_asked_again_for_the_next_utterance_and_a_missing_one_is_not` |
| T-IP-03-028 | 判定器が `en`。`en` の tier の答えは Han だけの文 `東京大学`（仮名なし）。偽の SenseVoice は印 zh、文 `东京大学`。`Tier::Rz` は文 `東京大学です`、`Tier::Pz` は文 `东京大学` を返す。`Policy::pair()`。同じ入力を `Policy::fast()` の router にも打つ | 決めた後の読み直しが `ja` を示し、文に仮名が無い（Han だけ） | pair は SenseVoice の裁きを呼ばず `ja` で読み直す | pair は `lang` が `ja`、`Tier::Sv` の呼びが 0 回、`Transcript.text` が `東京大学です`、`fallback` が `None`（判定器が `en` と答えたので、1 の経路）。fast は `lang` が `zh`、`Tier::Sv` の呼びが 1 回（specified：設計 10.4「`script_corrected_lang` が `ja` を返し、文に仮名が無い時の SenseVoice の裁き（`transcribe.rs:365` の `arbitrate_han_only`）は、pair では呼ばずに `ja` で読み直す」。fast が裁きを呼ぶ動きは今の `correct_by_script`：derived。裁きの答えの `zh`、`yue`、`ko` が 2 つの外に出ないことを見る） | 陰性、境界 | `a_han_only_reading_is_read_again_as_ja_without_asking_sensevoice_in_pair_mode` |
| T-IP-03-029 | 判定器が `zh`、偽の SenseVoice の印が zh。文を (a) `日本語abcdefg`（文字 10、仮名と漢字 3）。(b) `日本語だ abc, def 123`（文字 10、仮名と漢字 4。空白、読点、数字は文字に数えない）。`Policy::pair()`。`en` の tier は文 `yes` を返す | 仮名と漢字がちょうど 3 割（3 割以下の側の端）。文字の数え方：`is_alphabetic` の文字だけが分母 | 仮名と漢字が文字の 3 割を超えれば `ja`、3 割以下なら `en` | (a) は `lang` が `en`、`step` が `script`。(b) は `lang` が `ja`、`step` が `script`（specified：設計 10.4 の 3「3 割を超えれば `ja`、3 割以下なら `en`」と「3 割ちょうどを `en` に入れる」。比べは `script_corrected_lang` と同じ `>`。`>=` で比べる build は (a) で `ja` を返して落ちる。空白と記号と数字も分母に数える build は (b) が 4/17 になり `en` を返して落ちる。006 が 4/10 の `ja` の陽性対照） | 境界、陽性、陰性 | `three_tenths_exactly_is_en_and_the_denominator_counts_letters_only` |
| T-IP-03-030 | 判定器が `zh`、偽の SenseVoice の印が zh。文を (a) `안녕하세요`（Hangul だけ）。(b) `привет`（Cyrillic だけ）。`Policy::pair()`。`en` の tier は文 `hello` を返す | ラテン文字の外で、仮名も漢字も無い文。Hangul だけ、Cyrillic だけ | 3 割以下は全部 `en` | 2 入力とも `lang` が `en`、`step` が `script`。`ko` も `ru` も返らず、返る言語は `ja` か `en` の 2 つだけ（specified：設計 10.4 の 3「3 割以下の側には、ラテン文字だけの文、Hangul だけの文、Cyrillic だけの文、それらの混ざった文が入る」。007 がラテン文字だけの文の陽性対照。013 が決めた後の読み直しの `ko` を止める対） | 境界、陽性 | `a_text_of_hangul_only_or_cyrillic_only_is_en` |
| T-IP-03-031 | 判定器が `zh`、偽の SenseVoice の印が zh。文を (a) `hello 안녕하세요`（ラテン文字 5 と Hangul 5）。(b) `привет hello`（Cyrillic 6 とラテン文字 5）。(c) `ありがとう 안녕`（仮名 5 と Hangul 2）。`Policy::pair()`。`en` の tier は文 `hello`、`Tier::Rz` は文 `ありがとう` を返す | 文字の種類が混ざる文。仮名と漢字の割合は Hangul を分母に含む | 仮名と漢字が 3 割を超えれば `ja`、そうでなければ `en` | (a) と (b) は `lang` が `en`、`step` が `script`。(c) は `lang` が `ja`、`step` が `script`（5/7 で 3 割を超える）（specified：設計 10.4 の 3。Hangul は仮名にも漢字にも数えず、分母にだけ入る。030 と合わせて、Hangul の割合が高い文でも `ko` にならない。(c) は 3 割を超える側の対） | 境界、陽性 | `a_text_mixing_scripts_is_decided_by_the_share_of_kana_and_kanji_alone` |
| T-IP-03-032 | 判定の段が無い router（`None`）、`Policy::pair()`。偽の SenseVoice は 1 つ目の segment で印 en、文 `yes`、2 つ目の segment で印 ja、文 `はい`。`en` の tier は文 `yes`、`Tier::Rz` は文 `はい` を返す。live の segment を 2 つ続けて読む。別に、同じ入力を判定の段が無い `Policy::fast()` の router にも打つ | 判定の段が無く、session の言語が `en` | 判定の段が無い時は session の言語を答えに使わない | pair は 1 つ目が `en`（`sv_tag`）、2 つ目が `ja`（`sv_tag`）で、`Tier::Sv` の呼びが 2 回。fast は 2 つとも `ja`（`TIER0_LANG`、session の言語に従う）で、`Tier::Sv` の呼びが 0 回（specified：設計 10.4 の 2「判定の段が無い時に、session の言語（`router.rs:264-269` の `locked_lang`）を答えとして使わない」。session の言語を判定の答えに使う build は、pair の 2 つ目が `en` になり、`Tier::Sv` が 1 回になって落ちる。fast は今の `identify_lang` の動き：derived） | 境界、陰性 | `a_router_with_no_identifier_follows_sensevoice_and_not_the_session_language_in_pair_mode` |
| T-IP-03-033 | `Policy::pair()` と `Policy::fast()` の router それぞれに、先に ja の live の segment（判定器 `ja`）を 1 つ読ませた後、(a) `Router::partial(samples, Some("zh"))`、(b) `Router::partial(samples, None)` を呼ぶ。偽の `Tier::Pz` は文 `ni hao`、`Tier::Rz` は文 `こんにちは`、偽の SenseVoice は印 ja、文 `はい` を返す | 速報（draft）。手がかりが ja、en の外。手がかりが無い | pair は速報を変えない | (a) は両方が `ni hao` で、呼ばれた tier の列が両方とも `Tier::Pz` だけ。(b) は両方が `こんにちは` で、呼ばれた tier の列が両方で等しい（specified：設計 10.4「速報（draft）は変えない」「`route/draft.rs` は触らない」。derived：fast の答えと呼びの列は今の `Router::partial`。pair が手がかりを `ja` と `en` に絞る build は (a) で `ni hao` を返さず落ちる） | 陰性 | `a_draft_in_pair_mode_is_read_exactly_as_in_fast_mode` |
| T-IP-03-034 | `pair.rs`：(a) `Policy::pair()` の router に live の segment（`speech_s` 3.0 秒）を 2 つ読ませる。判定器の答えは `ja`、`fr`。偽の SenseVoice の答えは、1 つ目の segment が印 ja、文 `はい`、2 つ目が印 zh、文 `bonjour`。毎回 `session.pending` を読む。同じ入力を `Policy::balanced()` の router にも打つ。(b) `Policy::pair()` の router の `session.pending` を `Some(Pending { lang: "fr", count: 1 })` にして、live の segment を 1 つ読ませる（判定器 `fr`、SenseVoice の印 zh、文 `bonjour`） | pair は `pending` を読みも書きもしない（設計 10.4）。`DUAL_CONFIRM_LANGS` の外の `fr` | `pending` が動かない | (a) の pair は 2 つの segment の後とも `session.pending` が `None`。balanced は 2 つ目の後に `Some(Pending { lang: "fr", count: 1 })`。(b) は読んだ後も `Some(Pending { lang: "fr", count: 1 })` のまま（count が 2 にならず、`None` にもならない）（specified：設計 10.4「`session.pending` は pair では読みも書きもされない」と 10.11 の IP-S3 の条件 3。balanced の動きは `resolve_sticky` の既存の test：derived。balanced が `Some` を作ることが、`None` を数える式の陽性対照） | 陰性、陽性 | `pair_mode_neither_reads_nor_writes_the_pending_language_candidate` |
| T-IP-03-035 | `log.rs`：`Final` を 3 つ作る。(a) `id` 7 で `fallback` が `Some`（`step` が `sv_tag`、`lid` が `zh`）。(b) `id` 8 で `fallback` が `None`。(c) `id` 9 で `fallback` が `Some`（`step` が `sv_failed`、`lid` が答え無し）。1 つの `Final` から `asr.lang.fallback` の行を作る関数（名前は IP-S3 が決める。`endpoint_decided_line` の隣）に 1 つずつ渡す | `final` を出す所で、`fallback` を持つ final と持たない final | `final` 1 つにつき、`fallback` が在れば event が 1 件 | (a) は行が 1 つで、`event` が `asr.lang.fallback`、`id` が 7、`step` が `sv_tag`、`lid` が `zh`。(b) は行が 0 件。(c) は行が 1 つで level が `warn`、`id` が 9、`lid` が `null`（specified：設計 10.4「`final` を出す所で `asr.endpoint.decided`（`log.rs:244` の `endpoint_decided_line`）と並べて、その `final` の `id` を付けた log の event `asr.lang.fallback` を 1 件出す」。解いて持っただけで `final` にならなかった読みは、event を出さない（同じ節）。event は `Final` からだけ作るので、`final` にならなかった候補は行を作らない。持った候補が `final` になる時に `fallback` が `Final` まで届くことは、`pipeline.rs` の 036 から 039 が測る。候補ごとの出方は IP-2 の T-IP-02-016 から 018 が run の記録で見る。(a) が `Some` を数える陽性対照で、(b) が 0 件） | 陽性、陰性 | `a_final_that_fell_back_makes_one_language_fallback_line_with_its_id_and_one_that_did_not_makes_none` |
| T-IP-03-036 | `pipeline.rs` の `mod tests`：`Solved` を 2 つ作る。(a) 落ちの印の項目が `Some`（`step` が `sv_tag`、`lid` が `zh`）。(b) 同じ項目が `Some`（`step` が `sv_failed`、`lid` が答え無し）。それぞれを別の `endpoint::State<Solved>` に `hold(solved, 1000, 900)` し、続けて `plan_with_gap(100, 1000, WINDOW)` を呼ぶ | 持っておいた候補を返す枝（`Plan::Held`）。落ちの印の 2 つの形（`lid` が在る、答え無し） | 持った候補は、解いた時の落ちの印のまま返る | (a) (b) とも `plan` が `Plan::Held(held, 100)` で、`held.solved` の落ちの印の項目が入れた値と等しい。範囲の差（`plan_with_gap` の 2 つめの戻り）が `Some(0)`（specified：設計 10.4「持っておいた候補が後で `final` になる時（`Plan::Held`、`pipeline.rs:943-958`）は、候補を解いた時の `fallback` をそのまま使う」と 10.10 の段落の 1 つめの部品。`Plan::Held` の枝で `solved` を作り直して落ちの印を落とす build は、`None` を返して落ちる。pseudo：入れた値と比べる。前例は `endpoint.rs` の `a_kept_sentence_is_used_when_the_segment_ends_within_a_window_of_its_range`） | 陽性 | `a_held_candidate_returned_by_the_plan_keeps_the_language_fallback_it_was_solved_with` |
| T-IP-03-037 | 036 と同じ手順で、落ちの印の項目が `None` の `Solved`（落ちていない読み）を `hold` し、`plan_with_gap` を呼ぶ | 落ちていない候補 | 落ちていない候補に印を足さない | `plan` が `Plan::Held(held, 100)` で、`held.solved` の落ちの印の項目が `None`（specified：設計 10.4「落ちた時だけ値を持つ `fallback`」と、10.4 の「解いて持っただけ」の候補の扱い。落ちていない候補に既定の印を足す build は `Some` になって落ちる。036 が `Some` を保つ陽性対照） | 陰性 | `a_held_candidate_solved_without_a_language_fallback_is_returned_without_one` |
| T-IP-03-038 | `pipeline.rs` の `mod tests`：036 の (a) (b) の `Solved` を、`Solved` から `Final` を作る関数（route を要らない関数。名前は IP-S3 が決める。設計 10.10）に 1 つずつ渡す | `Final` を作る所。落ちの印の 2 つの形 | `Final` は `Solved` の落ちの印を写す | (a) (b) とも `Final` の `fallback` が入れた値と等しい。`text` と `lang` も `Solved` と同じ（specified：設計 10.10「`Solved` から `Final` を作る所が `fallback` を写し」。写さずに `None` を置く build は落ちる。pseudo：入れた値と比べる。035 は、この `Final` から行を作る所を測る） | 陽性 | `a_final_made_from_a_solved_reading_carries_its_language_fallback` |
| T-IP-03-039 | 038 の関数に、落ちの印の項目が `None` の `Solved` を渡す | 落ちていない読み | `Final` に印を足さない | `Final` の `fallback` が `None`（specified：設計 10.10「`fallback` の無い `Solved` からは `fallback` の無い `Final` ができる」。既定の印を足す build は `Some` になって落ちる。038 が `Some` を写す陽性対照） | 陰性 | `a_final_made_from_a_solved_reading_without_a_language_fallback_has_none` |
| T-IP-04-001 | 実物：pair の run の記録（`ip04_pair.jsonl` と log）と、同じ 30 clip を `--lang ja` の `lat-ja` と `--lang en` の `lat-en` で流した run の記録を、同じ機械で続けて取った物。`stream --repeat 3 --relay-port <中継の port>`（`--own-pid` 3 つと `--own-llama-port <訳の server の port>` は IP-2 の T-IP-02-001 と同じ形） | 3 つの run。clip の結び目は sha256 | 記録が欠けない。差の線は記録だけ | 記録の file に、結べた clip の数（30 が期待）、差（pair − `--lang`）の p50 と p95、`asr.utterance.timed` の `lid_ms` の p50 と p95、`L` と `L_tr` の p50 と p95 を書く。差が 150 ms と 300 ms の線を超えたら「線の外」と書き、test は落とさない。記録だけ。完成の合否に置かない（評議 2026-10-22）。仮の値。10.9.1 の置き直しの後に書き直す（p50 150 ms、p95 300 ms）。結べた clip が 0 の時は `Err`。`lat-ja` の ja 15 本と `lat-en` の en 15 本が `eval_real` の 30 本と sha256 で結ばれることは、2026-10-22 に `comm -12` で数えた（ja 15、en 15。`lat-ja` は ja 23 行） | 陽性 | `ip04_language_choice_adds_little_to_the_wait` |
| T-IP-04-002 | 道具：clip 1 つの pair の run の `L_asr` が 1000、1400、1200 ms（3 repeat）。同じ clip の `--lang` の run が 1000、1000、1100 ms | clip ごとの中央値の差 | 3 repeat の中央値を取る | 差が 200 ms（pair の中央値 1200 − `--lang` の中央値 1000）（specified：10.9 の IP-4「clip ごとの `L_asr` の中央値の差」。値は手で出した） | 陽性 | `the_delta_of_a_clip_is_the_median_of_its_repeats_in_one_run_minus_the_other` |
| T-IP-04-003 | 道具：clip 1 つの pair の run の `L_asr` が 1000、1000、2000 ms。`--lang` の run が 1000、1000、1000 ms | 中央値と平均の違い（外れ値が 1 つ） | 中央値 | 差が 0 ms（平均なら 333 ms）（specified、同上。002 が対の陽性） | 境界、陰性 | `a_median_of_three_is_not_a_mean` |
| T-IP-04-004 | 道具：clip 3 つの差が 149、150、151 ms | p50 の線 150 ms の境界の内側（nearest-rank で 2 番目 = 150） | p50 ≤ 150 ms（仮の値） | p50 の判定が「線の内」で、p50 は 150。p95 の判定は別に出る（specified：10.9 の IP-4。仮の値。10.9.1 の置き直しの後に書き直す） | 境界、陽性 | `a_delta_p50_of_150_ms_is_on_the_line` |
| T-IP-04-005 | 道具：clip 3 つの差が 150、151、152 ms | p50 の線の境界の外側（2 番目 = 151、1 ms 外） | p50 ≤ 150 ms（仮の値） | `check_delta` が `Err` で、p50 が 151 ms で 150 ms を超えることを言う（specified、同上。004 が対の陽性。仮の値。10.9.1 の置き直しの後に書き直す） | 境界、陰性 | `a_delta_p50_of_151_ms_is_over_the_line` |
| T-IP-04-006 | 道具：clip 20 個の差が 0 ms を 18 個、300 ms を 1 個、5000 ms を 1 個 | p95 の線 300 ms の境界の内側。nearest-rank で 19 番目 = 300、最大の 5000 は p95 を決めない | p95 ≤ 300 ms（仮の値） | p95 の判定が「線の内」で、p95 は 300（specified：10.9 の IP-4。pseudo：`awk` で `int((95*20+99)/100)` = 19 を打った。仮の値。10.9.1 の置き直しの後に書き直す） | 境界、陽性 | `a_delta_p95_of_300_ms_is_on_the_line_and_the_largest_clip_does_not_decide_it` |
| T-IP-04-007 | 道具：clip 20 個の差が 0 ms を 18 個、301 ms を 1 個、5000 ms を 1 個 | p95 の線の境界の外側（19 番目 = 301、1 ms 外） | p95 ≤ 300 ms（仮の値） | `check_delta` が `Err` で、p95 が 301 ms で 300 ms を超えることを言う（specified、同上。006 が対の陽性。仮の値。10.9.1 の置き直しの後に書き直す） | 境界、陰性 | `a_delta_p95_of_301_ms_is_over_the_line` |
| T-IP-04-008 | 道具：差が全 clip で 400 ms の合成の標本（clip 30 個） | 線の外（設計 10.9 の IP-4 の陰性対照） | p50 ≤ 150 ms かつ p95 ≤ 300 ms | `check_delta` が `Err` で、p50 と p95 の両方が線を超えることを言う（specified：10.9 の IP-4 の陰性。004 と 006 が `Ok` を返す陽性対照） | 陰性 | `a_synthetic_delta_of_400_ms_is_over_both_lines` |
| T-IP-04-009 | 道具：pair の run の clip の並びを、`--lang` の run の並びと逆にした記録。clip ごとの差は並びによらず同じ値 | 結び目が位置か sha256 か | sha256 で結ぶ | 差の一覧が、並びを替えない記録と同じ（specified：10.9 の IP-4「clip を sha256 で結び」。位置で結ぶ build は差が変わって落ちる） | 陰性 | `clips_are_joined_by_sha256_and_not_by_position` |
| T-IP-04-010 | 道具：pair の run だけに在る clip を 1 つ含む記録。別に、2 つの run に共通の clip が 0 の記録 | 片方にだけ在る clip。結べる clip が 0 | 結べない clip を名指す | 前者は結べた clip の数を 1 つ減らし、結べなかった clip の sha256 を名指す。後者は `Err`（0 件の p50 を作らない。implicit。002 が結べた clip の在る陽性対照） | 境界、陰性 | `a_clip_found_in_one_run_only_is_named_and_a_join_of_nothing_is_an_error` |
| T-IP-04-011 | 道具：`asr.utterance.timed` の行を 3 つ持つ pair の log（`lid_ms` が 90、100、110）。別に、`--lang` の run の log（`lid_ms` が 3 つとも 0） | pair の判定の時間。`--lang` の run は判定を飛ばす（`lid_ms` が 0。設計 10.3） | 時間を記録する | pair の log は `lid_ms` の p50 が 100、p95 が 110（nearest-rank で 2 番目と 3 番目）。`--lang` の log は p50 と p95 が 0 で、判定の時間が 0 の物と数えられる（specified：10.3。0 を「測れなかった」と読まないことは、行の数 3 を並べて見る） | 陽性、陰性 | `lid_ms_is_counted_in_a_pair_run_and_is_zero_in_a_forced_language_run` |
| T-IP-04-012 | 道具：repeat 1 の `L_asr` が 2000 ms の clip を 10 個、repeat 2 と 3 が 1000 ms の同じ clip | cold の repeat と warm の repeat（設計 H26：最初の英語の発話は認識器の読み込みを含む） | repeat 1 を別に出す | repeat 1 だけの p50 が 2000、repeat 2 と 3 の p50 が 1000 と別の欄に出る。全部を混ぜた p50 とも別に並ぶ（specified：cold と warm を別に測る。001 の記録の欄の形） | 境界 | `the_first_repeat_is_reported_apart_from_the_warm_repeats` |
| T-IP-05-001 | 実走：偽の翻訳 server `fake_llama_server -m translates.gguf --port <llm port>`、`hayamimi --input ws --mode pair --source file --serve <p1>`、`hayamimi-relay live --events http://127.0.0.1:<p1>/events --llm http://127.0.0.1:<llm port> --serve <p2>` を起こし、`chrome-headless-shell` で `http://127.0.0.1:<p1>/dashboard?relay=http://127.0.0.1:<p2>/events` の頁を先に開く（html は DevTools の `Runtime.evaluate` で読む。`--dump-dom` は読み込み時に dump し、`/events` を開いたままだと仮想時間が終わらない）。開いた後に、`lat-dialog` の行 7 から 10（ja、en、ja、en。行 6 の `end_sample` から行 10 の `end_sample` まで切った wav と、`start_sample` と `end_sample` を切った先頭からの sample に直した 4 行の manifest。設計 10.9.3 の 1）を `stream --out` で流す。`stream` が終わり、4 行に割り当たったすべての `final` の `id` に中継の `/events` が `of: final`、`settled: true` の訳を出した後に、頁の html を読む（設計 10.9.3 の 3。待ちの上限を過ぎたら、揃わない `id` を名指して落ちる）。判定の mode は `pair` | 頁を先に開く（final は新しい接続へ再送されない：`server.rs:557`）。4 行の言語が交互。1 行が 2 件以上の `final` に割れうる | 行ごとに、自分の言語の `final` が在り、割り当たった `final` の確定文の行の下に訳が 1 つ | `stream --out` の `.jsonl` と `.events.jsonl` に `support/dialog.rs` の `cells_of`（`build_samples` の割り当て）を当て、4 行のそれぞれで次の 2 つが成り立つ。(a) 割り当たった `final` のうち、`lang` が manifest の `lang` と同じ物が 1 件以上。(b) 割り当たったどの `final` も、`id` の同じ確定文の `row` が dump に 1 つ在り、その `row` の中に `tr` がちょうど 1 つ入る。`row` の外の `tr`（清書の欄の `RefineParagraph`）は数えない。1 行でも (a) が成り立たなければ、IP-5 の判定を出さず、行の番号と割り当たった `final` の `lang` を記録に書いて落ちる（specified：設計 10.9 の IP-5 と 10.9.3 の 2。`dashboard.rs` の `FeedRow` と `parts.rs` の `TrView` の形。数える式は 005 が持つ）。各 `tr` の訳の `status` は `ok` でも `held` でも 1 つと数える（V が偽の server の固定の文を止めることがある：設計 10.9 の IP-5 と 10.12 の H40。`tr` の数は `status` で変わらない）。`ok` の `tr` の文の照らしは 002 が行う | 陽性 | `ip05_one_page_shows_both_directions` |
| T-IP-05-002 | 001 の dump と、004 の読み手が読んだ `/events` の `of: final` の translation の `status` と `hold_reason`。dump の `tr` は、それを中に持つ `row` の `id` で translation に結ぶ | 向きと訳。`status` が `ok` の訳と `held` の訳 | 確定文の言語の逆の向きに訳が出る。`held` の訳は印と理由を出し、訳の文を出さない | 001 の (b) で数えたどの `tr` も、`tr-arrow` の text が、同じ `row` の `lang-badge` が ja なら `→en`、en なら `→ja`。4 行のそれぞれで、自分の言語の最後の `final` の `row` の `tr-arrow` が manifest の `lang` の逆（ja の行は `→en`、en の行は `→ja`）。`status` が `ok` の `tr` は、`tr-text` の要素を持ち、その text が、ja の行の訳なら偽の server の英語の固定文、en の行の訳なら日本語の固定文。`status` が `held` の `tr` は、`tr-mark` の要素を持ち、その text が `native/hayamimi-ui/data/strings.en.toml` と `strings.ja.toml` の `held`（`Translation withheld`、`訳を控えました`）のどちらかと等しく、`tr-text` の要素を持たない（event の `text` が何でも描かれない）。`tr-reason` の要素が出た `held` の `tr` は、その text が空でない（出ていない `held` の `tr` は落とさない）。`ok` の数が 0 の run は、文の照らしが 0 件で通ったと読まない。`held` の数が 0 の run は、`tr-mark` と `tr-text` の判定を出さず、通ったと読まない。どちらの run も、`status` の内訳（`ok` と `held` の数）を記録に書く（設計 10.9 の IP-5「`ok` の訳の文は偽の server の答え」と 10.5「訳の文（`tr-text`）は描かない」。設計 10.12 の H40 と同じ読み：判定した件数が 0 の判定は通ったと読まない）。矢印を `row` の `lang-badge` と比べ、manifest との一致は 001 の (a) が持つ（specified：IP-5 と設計 10.9.3 の 5。arrow と印の形は `parts.rs` の `TrView`。印の字は toml の値で、code の値ではない。`held` の行に `tr-text` を描く build は、`tr-text` を持つと読んで落ちる。道具の段の対照は 017 から 019） | 陽性 | `each_direction_shows_its_arrow_and_the_translation_the_server_made` |
| T-IP-05-003 | 001 と同じ run の間に、`?relay=` を付けない `http://127.0.0.1:<p1>/dashboard` も別に開いて dump を取る | 対照：`?relay=` の無い頁 | 訳が 0 | `row` の要素の数が 001 の dump の `row` の数と等しく 1 以上、`row` の中の `tr` の要素が 0（specified：10.5「`?relay=` が無い頁は今と同じに動く」。IP-5 の陰性対照）。頁が描かれていること（006）が前提 | 陰性 | `a_page_without_relay_in_its_address_shows_no_translation` |
| T-IP-05-004 | 001 の run の間に、独立した読み手が中継の `/events` を直に読む | 頁を通らない経路。中継は `of: final` と `of: refine` の訳を同じ `id` の字で出しうる | 行ごとに、自分の言語の最後の `final` に、反対の言語の settled な訳が在る | 4 行のそれぞれで、001 の (a) の `final` のうち割り当ての順で最後の物の `id` に、`of: final`、`settled: true`、`lang` が manifest の `lang` の逆（ja の行は `en`、en の行は `ja`）の translation が 1 件以上在る。`of: refine` の translation は数えない（specified：IP-5 の陽性対照と設計 10.9.3 の 2 の (b)、4。003 の 0 が「中継が出さなかった」のでなく「頁が読まなかった」ことを分ける） | 陽性 | `the_relay_publishes_four_settled_translations_for_the_four_lines` |
| T-IP-05-005 | 道具：手書きの html。`<div class="dash"><div class="row"><div class="row-text">x</div><div class="tr lang-ja"><span class="tr-arrow">→ja</span></div></div></div>` | class の token の比べ（`row-text` は `row` でなく、`tr-arrow` は `tr` でない） | token の完全一致 | `row` が 1、`tr` が 1、`tr-arrow` の text が `→ja`（部分文字列で数える build は `row` が 2、`tr` が 2 になって落ちる）。`row` の無い html は 0（specified：IP-5 の数え方） | 陽性、境界 | `the_page_counter_counts_whole_class_names_only` |
| T-IP-05-006 | 道具：`dash` の class を持たない html（描かれる前の頁、または白い頁） | 頁が描かれていない dump | 0 件を成果と読まない | `Err` で、頁が描かれていないことを言う（`tr` が 0 件の 003 を通さない。implicit。005 が `dash` の在る html を数える陽性対照） | 陰性 | `a_dump_of_an_unrendered_page_is_an_error_and_not_zero_counts` |
| T-IP-05-007 | `route.rs`：`?relay=http://127.0.0.1:8835/events` を読む関数（名前は IP-S2 が決める）に渡す。`page_for("/dashboard", 同じ query)` も呼ぶ | `?relay=` 付きの query（有効類） | query の値を読む | 読む関数が `Some("http://127.0.0.1:8835/events")`、`page_for` が `Page::Dashboard` のまま（specified：10.5） | 陽性 | `the_relay_address_is_read_from_the_query_and_the_page_stays_the_dashboard` |
| T-IP-05-008 | `route.rs`：読む関数に、空の query と `?show=final` | `?relay=` の無い query（無効類） | 無ければ読まない | 2 つとも `None`（specified：10.5「`?relay=` が無い頁は今と同じ」。007 が `Some` を返す陽性対照） | 陰性 | `a_query_without_a_relay_has_none` |
| T-IP-05-009 | `route.rs`：読む関数に `?relay=` | 値が空 | 空の値は中継でない | `None`（空の文字列の URL を開かない。implicit） | 境界、陰性 | `an_empty_relay_value_is_no_relay` |
| T-IP-05-010 | `route.rs`：`page_for("/", "?show=final&relay=http://127.0.0.1:8835/events")` と、読む関数に同じ query | 他の query と並ぶ。overlay の `show` が崩れない | 既存の読み方を変えない | `Page::Overlay(Show::Final)` のまま、読む関数は `Some(...)`（specified：既存の `Show::from_query` が他の key を飛ばす規則。`route.rs` の既存 test が全部通る） | 境界 | `a_relay_beside_another_query_key_changes_neither` |
| T-IP-05-011 | `route.rs`：読む関数に `?relay=https://192.168.0.5:8835/events` と `?relay=http://example.test:8835/events` | `https://` の値。127.0.0.1 の外の host（有効類） | `http://` か `https://` で始まれば中継の URL | 2 つとも `Some(値そのまま)`（specified：設計 10.5「decode した値が `http://` か `https://` で始まる時だけ中継の URL とし」「host は限らない」。設計 10.12 の H39。007 が `http://127.0.0.1` の陽性対照） | 陽性、境界 | `an_https_relay_and_a_relay_on_another_host_are_read` |
| T-IP-05-012 | `route.rs`：読む関数に、`?relay=` の値が (a) `javascript:alert(1)`。(b) `file:///etc/passwd`。(c) `data:text/plain,x`。(d) `ftp://127.0.0.1/events`。(e) `//127.0.0.1:8835/events`（scheme が無い）。(f) `127.0.0.1:8835/events`（scheme が無い）。(g) `HTTP://127.0.0.1:8835/events`。(h) `Https://127.0.0.1:8835/events` の 8 入力 | `http://` と `https://` の外の scheme、scheme の無い値、大文字を含む `http`、`https`（無効類） | 外は中継でない。scheme は字のとおりに比べ、大文字と小文字を区別する | 8 入力とも `None`（specified：設計 10.5「ほかの scheme（`javascript:`、`file:`、`data:` ほか）、key の無い query は、どれも中継なしとして今の頁と同じに動く」と「scheme は字のとおりに比べ、大文字と小文字を区別する。`HTTP://` や `Https://` で始まる値は、中継なしとして今の頁と同じに動く」。任意の URL を読む頁にしない。(g)(h) を `Some` にする build（比べの前に小文字へ直す build）は落ちる。011 が小文字の `Some` を返す陽性対照） | 陰性 | `a_relay_value_with_another_scheme_an_upper_case_scheme_or_none_is_no_relay` |
| T-IP-05-013 | `route.rs`：読む関数に、(a) `?relay=http%3A%2F%2F127.0.0.1%3A8835%2Fevents`。(b) `?relay=http://127.0.0.1:8835/events%3Fa%3Db`。(c) `?relay=http%3A%2F%2Fa.test%2Fe%3Fq%3Db+c` | percent-encode された値。値の中の `+` | `application/x-www-form-urlencoded` の規則で 1 回だけ decode する | (a) は `Some("http://127.0.0.1:8835/events")`。(b) は `Some("http://127.0.0.1:8835/events?a=b")`。(c) は `Some("http://a.test/e?q=b c")`（`+` が空白になる）（specified：設計 10.5「`application/x-www-form-urlencoded` の規則で 1 回だけ decode する」と「`URLSearchParams.get` と同じ読み」。出所は MDN の `URLSearchParams.get` と WHATWG URL。007 が decode の要らない値の陽性対照） | 陽性、境界 | `a_percent_encoded_relay_value_is_decoded_once_by_the_form_urlencoded_rules` |
| T-IP-05-014 | `route.rs`：読む関数に、(a) `?relay=javascript%3Aalert(1)`。(b) `?relay=http%253A%252F%252F127.0.0.1%253A8835%252Fevents`（2 回 encode）。(c) `?relay=%20http://127.0.0.1:8835/events`（decode すると先頭が空白）。(d) `?relay=%68ttp://127.0.0.1:8835/events`（`h` だけ encode）。(e) `?relay=http://127.0.0.1:8835/events%20`（decode すると後ろが空白）。(f) `?relay=http://127.0.0.1:8835/events%09`（decode すると後ろが tab） | decode の前と後で見え方が変わる値。1 回だけ decode する。decode した後の値で scheme を判定する。decode した後に後ろへ空白と制御文字が残る値 | decode した値が `http://` か `https://` で始まる時だけ。前後の空白と制御文字は削らない | (a)(b)(c) は `None`。(d) は `Some("http://127.0.0.1:8835/events")`。(e) は `Some("http://127.0.0.1:8835/events ")`、(f) は `Some("http://127.0.0.1:8835/events\t")`（specified：設計 10.5「decode した値の前後の空白と制御文字は削らない。前に空白の在る値は `http://` で始まらないので、中継なしである。後ろに空白の在る値は、字のまま `EventSource` に渡す」。(b) は 1 回の decode で `http%3A%2F%2F…` が残るので始まらず、2 回 decode する build は `Some` を返して落ちる。(d) は decode の前の文字で判定する build を落とす。(e)(f) は値を trim する build を、中身が変わって落とす。013 が 1 回の decode の陽性対照で、(c) が前の空白は削らず `None` になる対） | 境界、陰性 | `the_scheme_is_judged_on_the_value_decoded_exactly_once_and_the_value_is_not_trimmed` |
| T-IP-05-015 | `route.rs`：読む関数に、(a) `?relay=a&relay=http://127.0.0.1:8835/events`。(b) `?relay=http://127.0.0.1:8835/events&relay=a`。(c) `?xrelay=http://127.0.0.1:8835/events`、`?relay2=http://127.0.0.1:8835/events`、`?Relay=http://127.0.0.1:8835/events`。(d) `?relay`（`=` が無い） | key が 2 つ。key の名前が似ている。値の無い key | 最初の `relay` の値だけを読む。key は完全一致 | (a) は `None`（最初の値 `a` を読む）。(b) は `Some("http://127.0.0.1:8835/events")`。(c) の 3 入力は `None`。(d) は `None`（specified：設計 10.5 の `URLSearchParams.get` と同じ読み。`get` は最初の値を返し、`=` の無い key の値は空。007 が単独の `relay` の陽性対照） | 境界、陰性 | `only_the_first_relay_key_is_read_and_its_name_must_match_exactly` |
| T-IP-05-016 | `route.rs`：読む関数に、(a) `?relay=http://x.test/%zz`（不正な percent）。(b) `?relay=http://x.test/%`（末尾が `%`） | 不正な percent の並び | decode は失敗せず、不正な並びはそのまま残る | 2 入力とも `Some(値そのまま)`（panic も `None` も返さない）（specified：WHATWG URL の percent-decode は不正な並びをそのまま残す。設計 10.5 は `URLSearchParams.get` と同じ読みと書く。013 が正しい並びの陽性対照） | 境界 | `an_invalid_percent_sequence_in_the_relay_value_is_kept_as_it_is` |
| T-IP-05-017 | 道具：手書きの dump の html（外側に class が `dash` の要素）と、行ごとの `status`。行 1 は `status` が `ok`：`<div class="row"><div class="row-text">x</div><div class="tr lang-en"><span class="tr-arrow">→en</span><span class="tr-text">y</span></div></div>`。行 2 は `status` が `held`：`<div class="row"><div class="row-text">z</div><div class="tr lang-ja"><span class="tr-arrow">→ja</span><span class="tr-mark">訳を控えました</span><span class="tr-reason">w</span></div></div>` | `ok` と `held` が 1 行ずつ。`held` の `tr` が印と理由を持ち、文を持たない | `held` の `tr` は `tr-mark` を持ち、`tr-text` を持たない | 判定する関数が `Ok`。`ok` の数 1、`held` の数 1、`held` の判定が「通った」（specified：設計 10.5。002 の `held` の判定を出す式。018 が `Err` を返す側、019 が判定を出さない側の対の陽性対照） | 陽性 | `a_held_translation_with_a_mark_and_no_text_passes_the_page_check` |
| T-IP-05-018 | 道具：017 の `held` の行を、(a) `tr-text` の要素（text が `y`）を足した物。(b) `tr-mark` の要素を除いた物。(c) `tr-mark` の text を `x`（`strings.en.toml` と `strings.ja.toml` の `held` のどちらとも違う字）にした物 | `held` の行に文が在る。印が無い。印の字が違う | `held` の `tr` は印だけで、文は無い | 3 入力とも `Err` で、その行の位置と理由（`tr-text` が在る、`tr-mark` が無い、印の字が違う）を名指す（specified：設計 10.5「訳の文（`tr-text`）は描かない」と「控えた印（`tr-mark`）」。`held` の行へ文を描く build と、印を描かない build を、道具の段で落とす。017 が `Ok` を返す陽性対照） | 陰性 | `a_held_translation_that_shows_text_or_has_no_mark_fails_the_page_check` |
| T-IP-05-019 | 道具：017 の html と同じ形で、(a) 2 行とも `status` が `ok`（`held` の行が 0）。(b) 2 行とも `status` が `held`（`ok` の行が 0） | 判定する行が 0 件の判定 | 件数が 0 の判定を通ったと読まない | (a) は `Ok` で、`ok` の数 2、`held` の数 0、`held` の判定（`tr-mark` と `tr-text`）が「出さない」。(b) は `Ok` で、`ok` の数 0、`held` の数 2、`ok` の文の照らしが「出さない」。どちらも内訳（`ok` と `held` の数）が記録に入る（specified：設計 10.12 の H40「4 つとも止められた run では文の照らしが 0 件になる」と同じ読み。0 件の判定を「通った」と書く build は落ちる。017 が両方の判定を出す入力の陽性対照） | 境界、陰性 | `a_judgement_with_no_row_to_judge_is_not_issued_and_the_counts_are_recorded` |
| T-IP-06-001 | 受入：材料の 6 行の pinned manifest（`a1` から `a3` が ja、`b1` から `b3` が en）から、`--part` を ja、en の順に 2 つ、`--interleave`、間 10 ms で join を打つ | ja 3 本と en 3 本（本数が等しい） | 隣り合う 2 行の `lang` が違う | 6 行の `lang` が ja、en、ja、en、ja、en。`ref` が `a1`、`b1`、`a2`、`b2`、`a3`、`b3` の順。`start_sample` が 0、260、470、690、920、1160。`end_sample` が 100、310、530、760、1000、1250。wav の標本数が 1250（specified：IP-6 と 10.7。値は手で足した：長さの合計 450 と 5 つの間 5 × 160 = 800） | 陽性 | `ip06_interleaved_join_alternates_languages` |
| T-IP-06-002 | 001 の wav を `read_wav` で読み戻す | 間の中、clip の中、両端 | 間は 160 標本の 0、両端に間を置かない | 標本 [100, 260)、[310, 470)、[530, 690)、[760, 920)、[1000, 1160) が全部 0。0 番と 1249 番の標本は 0 でない。隣り合う clip の間（次の `start_sample` − 前の `end_sample`）が 160（specified：10.7「隣の clip の間が `--gap-ms` の sample 数」。pseudo：wav を走査する別の式） | 境界、陽性 | `an_interleaved_join_puts_exactly_the_gap_between_clips_and_none_at_the_ends` |
| T-IP-06-003 | 001 から `b3` を除いた manifest（ja 3 本、en 2 本） | 本数が違う：en が先に尽きる | 尽きた part は飛ばす | 5 行の `lang` が ja、en、ja、en、ja。`start_sample` が 0、260、470、690、920。wav の標本数が 1000（specified：10.7「part の clip が尽きたらその part を飛ばす」） | 境界 | `a_join_with_three_ja_and_two_en_ends_with_ja` |
| T-IP-06-004 | 001 から `a3` を除いた manifest（ja 2 本、en 3 本） | 本数が違う：ja が先に尽きる（003 の逆） | 尽きた part は飛ばす | 5 行の `lang` が ja、en、ja、en、en。`start_sample` が 0、260、470、690、920。`end_sample` の最後が 1010。wav の標本数が 1010（specified、同上） | 境界 | `a_join_with_two_ja_and_three_en_ends_with_two_en` |
| T-IP-06-005 | 001 と同じ manifest と part を、`--interleave` なしで join する | 対照：`--interleave` の無い join | 交互にならない | `lang` が ja、ja、ja、en、en、en。`check_alternates` が `Err` で、隣り合う `lang` が同じ最初の組を位置で言う。001 の出力を同じ関数に渡すと `Ok`（specified：IP-6 の陰性対照） | 陰性 | `a_join_without_interleave_does_not_alternate` |
| T-IP-06-006 | ja の本数 na と en の本数 nb を、それぞれ 0 から 4 まで振った 24 通り（na と nb が両方 0 は除く。012 が持つ）。clip の長さは全部違う | 本数の組み合わせの全体 | 規則が全域で成り立つ | 24 通りすべてで、`lang` の並びが別に書いた参照（先頭から ja、en の順に 1 本ずつ取り、尽きた側を飛ばす）と一致し、各言語の中の `ref` の順が part の中の順と同じで、wav の標本数が clip の長さの合計 + (n − 1) × 間（pseudo：参照は interleave の code を通らない） | 性質 | `interleave_is_round_robin_and_keeps_each_languages_order` |
| T-IP-06-007 | ja の part 1 つだけ（3 本）で `--interleave` | part が 1 つ | 1 つなら塊でつなぐのと同じ | manifest と wav が、`--interleave` なしの join と bytes で等しい（derived：part が 1 つなら順が変わらない） | 境界 | `interleave_with_one_part_gives_the_same_join_as_without_it` |
| T-IP-06-008 | ja 2 本、en 2 本、zh 1 本（`c1`）の 3 つの part を、ja、en、zh の順に `--interleave` | part が 3 つ。真ん中の part が先に尽きない、最後の part が先に尽きる | part の順に 1 本ずつ | `lang` が ja、en、zh、ja、en（specified：10.7「各 `--part` から 1 本ずつ順に取り」） | 境界 | `three_parts_are_taken_in_turn_and_an_exhausted_part_is_skipped` |
| T-IP-06-009 | 001 の manifest の各行 | 行の項目 | 行ごとに 4 項目を持つ | 6 行すべてが `lang`、`start_sample`、`end_sample`、`sha256`、`wav`、`ref` を持ち、`start_sample` < `end_sample`。`sha256` が元の wav の bytes の sha256 と等しい（specified：10.7。pseudo：`support::sha256_hex` で別に数える） | 陽性 | `every_row_of_an_interleaved_manifest_carries_lang_span_and_sha256` |
| T-IP-06-010 | pin で固めた `a2` の wav の値を書き換えてから、`--interleave` で join を打つ | wav が pin の sha256 と食い違う | pin の sha256 と一致しない clip は拒む | `Err` で、メッセージが `sha256` を言い、出力の manifest も wav も書かれない（specified：既存の `a_clip_that_no_longer_matches_its_sha256_is_refused` と同じ式を `--interleave` で打つ） | 陰性 | `a_clip_that_no_longer_matches_its_sha256_is_refused_under_interleave` |
| T-IP-06-011 | ja の行が 0 の manifest で、`--part` を ja、en の順に取り、en が 3 本 | 最初から尽きている part | 空の part は飛ばす | `lang` が en、en、en の 3 行で、`Err` にならない（derived：「尽きたら飛ばす」を、最初から尽きた part に当てる） | 境界 | `a_part_with_no_clips_of_its_language_is_skipped` |
| T-IP-06-012 | ja の行も en の行も 0 の manifest で、`--interleave` | 全部の part が空 | clip が 1 本も無ければ拒む | `Err`（`EvalError::Manifest`）。出力の file が書かれない（specified：既存の `parts_with_no_clips_are_refused` と同じ。011 が空でない part の在る陽性対照） | 陰性 | `parts_that_hold_no_clips_are_refused_under_interleave` |
| T-IP-06-013 | 001 と同じ入力を `--gap-ms 0` で | 間の下端（0 ms） | 間 0 標本 | `start_sample` が 0、100、150、210、280、360、`end_sample` が 100、150、210、280、360、450、wav の標本数が 450（specified：間が `--gap-ms` の標本数。値は手で足した：長さの合計 450） | 境界 | `a_zero_gap_puts_clips_back_to_back` |
| T-IP-06-014 | 実物：10.7 の command で作った `testdata/manifests/lat-dialog.json` と `testdata/lat-dialog.wav` | 実物の録音。間 800 ms = 12800 標本 | 交互、間が 12800 | 30 行で、`lang` が ja から始まって ja と en が 15 ずつ交互。各行の次の `start_sample` − （その行の `start_sample` + 元の wav の標本数）が 12800（元の wav の標本数は `read_wav` で別に数える）。最初の `start_sample` が 0。wav の標本数が clip の長さの合計 + 29 × 12800。各行の `sha256` が元の wav の bytes の sha256 と等しい（specified：10.7。pseudo：元の wav を読む別の式） | 陽性 | `lat_dialog_alternates_ja_and_en_with_an_800_ms_gap` |
| T-IP-06-015 | `hayamimi-eval fetch-set` を `--interleave`、`--part manifest:<m>:ja`、`--part manifest:<m>:en`、`--gap-ms 10`、`--out <o>`、`--wav-out <w>` で起こす。`--concat` は付けない。`<m>` は材料の 6 行の pinned manifest | `--concat` の無い `--interleave`（無効な組。clap の `requires`） | `--interleave` は `--concat` を要る | exit code 2。stderr が `--concat` を名指す（clap の `MissingRequiredArgument` の使い方の誤り）。`<o>` も `<w>` も書かれない（specified：設計 10.7「`--concat` の無い `--interleave` は、何も読まずに clap の使い方の誤り（`ErrorKind::MissingRequiredArgument`、exit code 2）で止まる」と 10.2.1 の clap の `Arg::requires`。001 と同じ引数に `--concat` を足した形が exit code 0 で `<o>` と `<w>` を書く対照） | 陰性 | `interleave_without_concat_is_a_usage_error_and_writes_nothing` |
| T-IP-06-016 | 2 入力。(a) `fetch-set --pin <m> --interleave --out <o>`（`--concat` は付けない）。(b) `fetch-set --pin <m> --concat --interleave --part manifest:<m>:ja --part manifest:<m>:en --wav-out <w> --out <o>` | `--pin` と並べた `--interleave`。黙って無視される形 | `--pin` の run は交互の join にならない | 2 入力とも exit code 2 で、`<o>` が書かれない。(a) は stderr が `--concat` を名指し、(b) は stderr が `--pin` と `--concat` の衝突を名指す（specified：設計 10.7「`--pin` と並べた時に黙って無視する形は採らない。打った者が交互につないだつもりの manifest が、ただの pin の写しになるからである」。(b) は既存の `conflicts_with`：derived。`--pin` だけの run が exit code 0 で `<o>` を書く対照は既存の pin の test） | 陰性、境界 | `interleave_beside_pin_is_a_usage_error_and_never_a_silent_pin` |
| T-IP-07-001 | 実走：IP-2 と同じ起こし方で `stream --wav testdata/lat-short.wav --repeat 3 --relay-port 8835 --own-pid <hayamimi の pid> --own-pid <中継の pid> --own-pid <訳の server の pid> --own-llama-port <訳の server の port> --out ip07_pair.jsonl` を流す | `lat-short` の 20 発話 × 3 repeat | 記録が欠けない。取り違えの数は記録だけ | `ip07_pair.jsonl` が 60 行（repeat 1 から 3 と utt 1 から 20 が 1 回ずつ）で、IP-2 と同じ数え方（`tests/support` の同じ関数）で取り違えた発話の数を向きごとに記録する。取り違えた発話が 6 を超えても落とさない。記録だけ。完成の合否に置かない（評議 2026-10-22）。仮の値。10.9.1 の置き直しの後に書き直す（線は 6）。`ja` でも `en` でもない final の数も記録する | 陽性 | `ip07_short_replies_are_read_in_their_own_language` |
| T-IP-07-002 | 実物：`testdata/manifests/lat-short.json` | 実物の manifest。1 秒未満の長さ | 20 行すべてが 1 秒未満 | 行が 20、`lang` が ja 10 と en 10。20 行すべてで `end_sample − start_sample` が 16000 未満（specified：10.9 の IP-7 の陽性対照。値は 003 と同じ式で数える）。長さの最小と中央値と最大を記録に書く | 陽性 | `lat_short_has_twenty_replies_each_shorter_than_one_second` |
| T-IP-07-003 | 道具：manifest の合成の行で、`end_sample − start_sample` が 15999 の行と 16000 の行 | 1 秒の境界の両側 | 16000 標本未満 | 15999 は「1 秒未満」、16000 は「1 秒未満でない」。後者を含む manifest は `Err` で、その行を名指す（specified：10.12 の H20「1 秒より短い返事」。16000 ちょうどは 1 秒で、短いとしない） | 境界、陰性 | `a_reply_of_15999_samples_is_short_and_one_of_16000_is_not` |
| T-IP-07-004 | 合成の記録：20 発話（manifest が ja と en の交互）の全 final の `lang` が `ja` | `--lang ja` の run の形（10.9 の IP-7 の陰性対照） | en の発話が全部取り違え | 取り違えた発話が 10、向きは `en_as_ja` が 10。10 は線の 6 を超える（specified：10.9 の IP-7 の陰性。IP-2 の 007 と同じ関数） | 陰性 | `a_run_forced_to_ja_mixes_up_ten_short_english_replies` |
| T-IP-07-005 | 実物：`testdata/manifests/lat-short.json` の `lang` の並び | 交互 | 交互 | `lang` が ja から始まる交互で、切り替えが 19（IP-2 の 014 の関数。specified：10.12 の H20「`--interleave` で交互につないだ」） | 陽性 | `lat_short_alternates_ja_and_en_ten_each` |
| T-IP-07-006 | 実物：`lat-short.json` の各行の `ref` | `lang` と返事の文字の一致 | ja の返事は仮名を含み、en の返事は仮名も漢字も含まず英字を含む | ja の 10 行の `ref` がすべて `has_kana`（`lid.rs`）、en の 10 行の `ref` がすべてラテン文字だけ（pseudo：manifest の `lang` を、返事の文字で別に確かめる。`lang` の付け間違いと声の取り違えを止める。声が正しいかは機械で測れない：設計 10.12 の H20） | 陽性、陰性 | `every_reply_of_lat_short_is_written_in_the_script_of_its_language` |
| T-IP-07-007 | 実物：`lat-short.json` の各行の wav | 固めた bytes と標本の形 | pin と一致、16 kHz | 20 行すべてで、`sha256` が wav の bytes の sha256 と等しく、wav の標本の率が 16000、channel が 1。1 行を書き換えた複製を join すると `Err` で `sha256` を言う（specified：既存の join の検査。IP-6 の 010 と同じ式が陰性） | 陽性、陰性 | `every_wav_of_lat_short_matches_its_sha256_and_is_16_khz` |
| T-IP-07-008 | 取り違えた発話の数 6 と 7 を、線の比べの関数に渡す | 線の境界（仮の値 6）の両側 | 数 ≤ 6 | 6 は「線の内」、7 は「線の外」を返し、どちらも `Ok`（記録に書く関数で、落とさない）。記録の file に線の値 6 が書かれる（specified：評議 2026-10-22。仮の値。10.9.1 の置き直しの後に書き直す） | 境界 | `six_mixed_up_replies_are_on_the_line_and_seven_are_over_it` |

| T-IP-08-001 | 受入：材料の IP-8 の fixture（4244 の cmdline が `llama-server`、`-m`、`model.gguf`、`--port`、`18083`）で、wav 1 秒の `stream` を `--own-pid 4242 --own-pid 4243 --own-pid 4244 --own-llama-port 18083` で起こす | 訳の server が 18084 の外の port（pair の run の形）。3 つの comm が揃う | `--port` の次の語が、`--own-llama-port` の値を 10 進で書いた字に等しい | 始まる。exit code が 2 でも 3 でもない、`stream.machine.refused` と `stream.machine.aborted` が 0 件、偽の `/ingest` が受けた音声の byte が 1 以上。machine の記録の行 1 の `own` が `[{"pid":4242,"comm":"hayamimi"},{"pid":4243,"comm":"hayamimi-relay"},{"pid":4244,"comm":"llama-server"}]` に等しい（渡された順）。全行の `own_cpu_ticks` が JSON の整数 3751（specified：設計 10.9 の IP-8 と 10.7.1。3751 は RP-83 の S83o の stat の値の和） | 陽性 | `ip08_the_own_llama_server_is_checked_on_the_port_the_run_names` |
| T-IP-08-002 | 001 の fixture の 4244 の cmdline の `18083` を `18084` にし、`--own-llama-port 18084` で起こす | 窓の形（port 18084）。今まで固定だった値を引数で名指した形 | 001 と同じ | 001 と同じに始まる。`stream.machine.refused` が 0 件、行 1 の `own` が 001 と同じ 3 つ（specified：設計 10.9 の IP-8 の陽性対照「同じ fixture の cmdline を `18084` にし、`--own-llama-port 18084` で起こすと同じく受けられる（窓の形）」と 10.7.1「窓の run は `--own-llama-port 18084` を渡す」。003 が port の食い違いを拒む対） | 陽性 | `a_server_on_18084_is_accepted_when_the_run_names_18084` |
| T-IP-08-003 | 001 の fixture の 4244 の cmdline を `llama-server`、`--port`、`8080` にし、`--own-llama-port 18083` で起こす | comm が `llama-server` で、別の app の port | 名指した port と違えば拒む | exit code 2。`stream.machine.refused` が 1 件（level `warn`）で `reason` が `own_pid`、`own_fault` が `port`、`own_pid` が 4244（JSON の整数）。偽の `/ingest` が受けた音声の byte は 0（specified：設計 10.9 の IP-8 の陰性対照「`stream.machine.refused` が 1 件（`reason` が `own_pid`、`own_fault` が `port`）、送った byte が 0、exit code が 2」。この run の物でない server の CPU 時間を run の分に数えない目的。001 が同じ引数で受けられる陽性対照） | 陰性 | `a_server_on_another_port_than_the_one_named_stops_the_stream_before_any_audio` |
| T-IP-08-004 | 2 入力。(a) 001 の起動から `--own-llama-port 18083` を除く（`--own-pid` は 3 つ）。(b) 001 の起動から `--own-llama-port 18083` と `--own-pid 4242`、`--own-pid 4243` を除く（`--own-pid` は 4244 の 1 つ） | `--own-pid` が在って `--own-llama-port` が無い。pid が 1 つでも | 既定の値を置かない | どちらも exit code 2。stderr が `--own-llama-port` を名指す。`stream.machine.refused` が 0 件で、machine の記録の file が書かれず、偽の `/ingest` が受けた音声の byte は 0（specified：設計 10.7.1「`--own-pid` が 1 つでも在るのに `--own-llama-port` が無い時と、`--own-pid` が無いのに `--own-llama-port` が在る時は、clap の使い方の誤り（exit code 2）で、`stream.machine.refused` も machine の記録も書かずに止まる」と「18084 を既定の値にする形は採らない」。18084 を既定に持つ build は (a) で起動して落ちる。001 が両方を渡す陽性対照） | 陰性、境界 | `own_pid_without_own_llama_port_is_a_usage_error_and_writes_no_record` |
| T-IP-08-005 | 001 の起動から `--own-pid` の 3 つを除き、`--own-llama-port 18083` だけを渡す | `--own-llama-port` が在って `--own-pid` が無い | 互いを要る | exit code 2。stderr が `--own-pid` を名指す。`stream.machine.refused` が 0 件で、machine の記録の file が書かれず、音声の byte は 0（specified：設計 10.7.1「`--own-pid` が 1 つでも在るのに `--own-llama-port` が無い時と、`--own-pid` が無いのに `--own-llama-port` が在る時は、clap の使い方の誤り（exit code 2）で、`stream.machine.refused` も machine の記録も書かずに止まる」。`--own-pid` も `--own-llama-port` も渡さない起動は RP-83 の T-RP-83-087 が「今のまま動く」と持つ。001 が対の陽性対照） | 陰性、境界 | `own_llama_port_without_own_pid_is_a_usage_error_and_writes_no_record` |
| T-IP-08-006 | 2 入力。(a) 001 の fixture の 4244 の cmdline を `18084` にし、`--own-llama-port 18083` で起こす。(b) cmdline を `18083` のままにし、`--own-llama-port 18084` で起こす | 名指した port と cmdline の port が食い違う。今まで固定だった 18084 が名指した値でない場合 | 名指した値に等しい時だけ受ける | どちらも 003 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified：設計 10.7.1「比べは今のまま、語 `--port` がちょうど 1 つ在り、その次の語がこの引数の値を 10 進で書いた字と完全に等しい時だけ通す」。(a) は 18084 を今でも受ける build を、(b) は cmdline の 18083 を固定で受ける build を落とす。001 と 002 が一致する側の陽性対照） | 境界、陰性 | `the_port_in_the_command_line_must_equal_the_port_named_and_not_a_fixed_one` |
| T-IP-08-007 | 2 入力。001 の fixture の 4244 の cmdline の `18083` を、(a) `180830` にする。(b) `118083` にする。`--own-llama-port 18083` | 名指した port を前や後ろに含む別の語（完全一致でない） | 完全一致 | どちらも 003 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified：設計 10.7.1。RP-83 の T-RP-83-072 の新しい port での形。部分一致か前方一致で受ける build は、ここで受けて落ちる。001 が完全一致の陽性対照） | 境界、陰性 | `a_port_that_only_contains_the_named_port_stops_the_stream` |
| T-IP-08-008 | 001 の fixture の 4244 の cmdline を `llama-server`、`--port=18083` の 2 語にする。`--own-llama-port 18083` | `=` でつなぐ 1 語の形 | `--port` の次の語が名指した port | 003 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified：設計 10.7.1 は `--port=<port>` の 1 語の形を「今のとおり `port` で拒む」と書く。T-RP-83-083 の新しい port での形） | 境界、陰性 | `a_port_flag_joined_with_equals_stops_the_stream_on_the_named_port` |
| T-IP-08-009 | 3 入力。001 の fixture の 4244 の cmdline を、(a) `llama-server`、`--port`、`18083`、`--port`、`18083`。(b) `llama-server`、`--port`、`8080`、`--port`、`18083`。(c) `llama-server`、`--port`、`18083`、`--port`、`8080` にする。`--own-llama-port 18083` | `--port` が 2 つ。両方が名指した port、後ろだけ、前だけ | `--port` がちょうど 1 つ | 3 入力とも 003 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified：設計 10.7.1 は `--port` が 2 つ在る形を「今のとおり `port` で拒む」と書く。(a) は名指した port に等しければ数を見ない build を、(b) は最後の `--port` だけを見る build を、(c) は最初の `--port` だけを見る build を落とす） | 境界、陰性 | `two_port_flags_stop_the_stream_on_the_named_port` |
| T-IP-08-010 | 001 の fixture の 4244 の cmdline を `llama-server`、`-m`、`model.gguf`、`--port` の 4 語にする（最後の語が `--port`）。`--own-llama-port 18083` | `--port` が最後の語 | 次の語が要る | 003 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified：設計 10.7.1 は `--port` が最後の語の形を「今のとおり `port` で拒む」と書く。T-RP-83-073 の新しい port での形） | 境界、陰性 | `a_port_flag_with_no_value_stops_the_stream_on_the_named_port` |
| T-IP-08-011 | 2 入力。001 の fixture の 4244 の cmdline を、(a) `llama-server`、`--port`、`8080`、`--parallel`、`18083`。(b) `llama-server`、`18083`、`--port`、`8080` にする。`--own-llama-port 18083` | 名指した port が語として在るが、`--port` の次の語でない | `--port` の次の語 | どちらも 003 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified：設計 10.7.1。cmdline の中に port の字が在れば受ける build は、ここで受けて落ちる。T-RP-83-074 の新しい port での形） | 境界、陰性 | `a_named_port_that_is_not_the_value_of_port_stops_the_stream` |
| T-IP-08-012 | 001 の fixture の 4244 の cmdline の `18083` を `018083`（先頭に 0 を付けた綴り）にする。`--own-llama-port 18083` | 同じ数を別の綴りで書いた語 | 10 進の字の完全一致 | 003 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified：設計 10.7.1「この引数の値を 10 進で書いた字と完全に等しい」。`018083` は `18083` を 10 進で書いた字と等しくない。数に読んで比べる build は、ここで受けて落ちる。001 が `18083` の陽性対照） | 境界、陰性 | `a_port_written_with_a_leading_zero_does_not_match_the_named_port` |
| T-IP-08-013 | 001 の起動の `--own-llama-port 18083` を `--own-llama-port abc` にする | 数でない port（引数の誤り） | 拒みの印は event でない | exit code 2。stderr が `--own-llama-port` を名指す。`stream.machine.refused` が 0 件で、machine の記録の file が書かれず、音声の byte は 0（specified：設計 10.7.1「数でない字（`abc`）は、clap の使い方の誤り（exit code 2）で、`stream.machine.refused` も machine の記録も書かずに止まる」。T-RP-83-079 の `--own-pid abc` と同じ型の行。引数の誤りの exit code も 2 なので、拒みは event の 0 件で確かめる。001 が数の `18083` で受けられる陽性対照） | 陰性 | `a_non_numeric_own_llama_port_is_a_usage_error_and_not_a_refusal` |
| T-IP-08-014 | 道具と実物：`native/hayamimi-eval/src/machine.rs` の `//!` と `///` の行、`native/hayamimi-eval/src/cli.rs` の `///` の行、`docs/results/e2e/README.md` の `--own-pid` を含む行を読む。式は「`18084` を含み、`--own-llama-port` を含まない行」。陽性対照として、同じ式を合成の text（`/// on port 18084` と `/// --own-llama-port 18084`）に掛ける | 直す 4 か所（machine.rs の `//!`、`OwnFault::Port` の doc、cli.rs の `--own-pid` の説明、README の `--own-pid` の 2 行）を引く consumer | 18084 を固定の port として書く行が無い | 実物は 3 つの file で該当 0 行。合成の text は 1 行（1 つめ）が該当し、2 つめは該当しない（specified：設計 10.7.1 は、4 か所の「`--port 18084`」を「`--own-llama-port` で名指した port」に書き替えると決め、書き替えるのは IP-8 を実装する IP-S1 の席で、code と同じ commit にすると書く。逐語を変えたら、引用する全 consumer を数える。RP-83 の rp83.rs と s83.rs の 18084 は `--own-llama-port 18084` の値として残るので、この式の範囲に入れない） | 陽性、陰性 | `no_comment_or_readme_line_names_18084_as_the_fixed_port_of_the_check` |
| T-IP-08-015 | 2 入力。001 の fixture の 4244 の cmdline を (a) `llama-server`、`--port`、`18083`（001 のまま）。(b) `llama-server`、`--port`、`8080` にし、どちらも `--own-llama-port 0` で起こす（cmdline に `--port 0` を置かない） | 値 `0`（`u16` の下端）。引数の読みでは止まらず、cmdline の比べで拒まれる | `0` は値として受け、cmdline の `--port` の次の語が `0` の時だけ通す | 2 入力とも exit code 2。`stream.machine.refused` が 1 件（level `warn`）で `reason` が `own_pid`、`own_fault` が `port`、`own_pid` が 4244。偽の `/ingest` が受けた音声の byte は 0（specified：設計 10.7.1「`0` は値として受け、cmdline の `--port` の次の語が `0` の時だけ通す。…0 を名指した run は `port` で拒まれる。0 を引数の読みで止める範囲は置かず」。0 を引数の読みで止める build は、`stream.machine.refused` が 0 件になって落ちる。001 が名指した port と cmdline が等しく受けられる陽性対照、003 が 8080 を拒む対） | 境界、陰性 | `an_own_llama_port_of_zero_is_read_and_stops_the_stream_at_the_port_check` |
| T-IP-08-016 | 001 の fixture の 4244 の cmdline の `18083` を `65535` にし、`--own-llama-port 65535` で起こす | `u16` の上端（受ける側の端） | 範囲の中の最大 | 001 と同じに始まる。`stream.machine.refused` が 0 件、行 1 の `own` が 001 と同じ 3 つ（specified：設計 10.7.1「値の型は、`stream` のほかの port の引数と同じ `u16`」。017 が 1 つ上を拒む対） | 境界、陽性 | `the_largest_u16_port_is_accepted_when_the_server_listens_on_it` |
| T-IP-08-017 | 001 の起動の `--own-llama-port 18083` を `--own-llama-port 65536` にする（cmdline は 001 のまま `18083`） | `u16` の上端の 1 つ上 | `u16` の外は引数の読みで止まる | exit code 2。stderr が `--own-llama-port` を名指す。`stream.machine.refused` が 0 件で、machine の記録の file が書かれず、音声の byte は 0（specified：設計 10.7.1「`65536` … は、clap の使い方の誤り（exit code 2）で、`stream.machine.refused` も machine の記録も書かずに止まる」。`u32` か `usize` で読む build は、cmdline の比べまで進んで `stream.machine.refused` を 1 件出して落ちる。016 が 1 つ下を受ける陽性対照） | 境界、陰性 | `an_own_llama_port_one_above_u16_is_a_usage_error_and_not_a_refusal` |
| T-IP-08-018 | 2 入力。001 の起動の `--own-llama-port 18083` を (a) `--own-llama-port -1`（2 語）。(b) `--own-llama-port=-1`（1 語）にする | 負の数。値の渡し方の 2 つの綴り | 負の数は引数の読みで止まる | 2 入力とも 017 と同じ（exit code 2、stderr が `--own-llama-port` を名指す、`stream.machine.refused` が 0 件、machine の記録の file が書かれない、音声の byte は 0）（specified：設計 10.7.1「負の数（`--own-llama-port -1` と `--own-llama-port=-1`）…は、clap の使い方の誤り」。符号つきの型で読み、cmdline の比べまで進む build は落ちる。001 が数を受ける陽性対照） | 陰性 | `a_negative_own_llama_port_is_a_usage_error_in_both_spellings` |
| T-IP-08-019 | 001 の起動の `--own-llama-port 18083` を `--own-llama-port` と空の語（長さ 0 の引数）にする | 空の値 | 空は引数の読みで止まる | 017 と同じ（exit code 2、stderr が `--own-llama-port` を名指す、`stream.machine.refused` が 0 件、machine の記録の file が書かれない、音声の byte は 0）（specified：設計 10.7.1「…空の値は、clap の使い方の誤り」。空を 0 か既定の値に読む build は、cmdline の比べまで進んで `stream.machine.refused` を 1 件出して落ちる。001 が非空の値を受ける陽性対照） | 境界、陰性 | `an_empty_own_llama_port_is_a_usage_error` |
| T-IP-08-020 | 2 入力。001 の起動の `--own-llama-port 18083` を (a) `--own-llama-port 18083 --own-llama-port 18084`（cmdline は 001 のまま `18083`）。(b) `--own-llama-port 18083 --own-llama-port 18083` にする | 引数を 2 回渡す。値が違う、値が同じ | 後の値を黙って採らない | 2 入力とも exit code 2。stderr が `--own-llama-port` を名指す。`stream.machine.refused` が 0 件で、machine の記録の file が書かれず、音声の byte は 0（specified：設計 10.7.1「引数を 2 回渡す形は、clap の `ArgAction::Set` の既定どおり `ArgumentConflict` の使い方の誤り（exit code 2）で止まる。後の値を黙って採る形は採らない」。後の値を採る build は (a) で 18084 と cmdline の `18083` を比べて refused を 1 件出し、最初の値を採る build は (a) で受けて始まり、どちらも落ちる。(b) は同じ値でも拒む。001 が 1 回だけ渡す陽性対照） | 境界、陰性 | `an_own_llama_port_given_twice_is_a_usage_error_and_neither_value_is_taken` |
| T-IP-08-021 | 001 の fixture（cmdline は `--port`、`18083`）を `--own-llama-port 018083`（先頭に 0 を付けた綴り）で起こす | 先頭に 0 のある引数 | 引数は数に読み、比べる字は 10 進で書いた `18083` | 001 と同じに始まる。`stream.machine.refused` が 0 件、行 1 の `own` が 001 と同じ 3 つ（specified：設計 10.7.1「先頭に 0 のある引数は 18083 と読む。cmdline と比べる字は、値を 10 進で書いた `18083` で、引数の綴りではない」。引数の綴りの字をそのまま比べる build は、cmdline の `18083` と違うと読んで拒み、落ちる。012 が cmdline の側に 0 を付けた拒みの対） | 境界、陽性 | `an_own_llama_port_written_with_a_leading_zero_is_read_as_the_number` |
| T-IP-08-022 | 001 の fixture の 4244 の cmdline の `18083` を `018083` にし、`--own-llama-port 018083` で起こす | 引数と cmdline が同じ綴り（先頭に 0） | 比べる字は引数の綴りでなく、10 進で書いた字 | 003 と同じ。`own_fault` は `port`、`own_pid` は 4244（specified：設計 10.7.1「cmdline の語が `018083` の server は、`18083` を名指しても通らない」。引数の綴りと cmdline の語の綴りを比べる build は、ここで受けて落ちる。021 が引数の側の 0 を読んで通る対、012 が cmdline の側だけに 0 を付けた対） | 境界、陰性 | `a_command_line_port_written_with_a_leading_zero_does_not_match_even_the_same_spelling` |

**設計の在り処**（この席は受入の条件を足さない。設計の席に問うていた 11 の事項と、`machine.rs` の `LLAMA_PORT` の直しの案は、設計の 10 節が決めた。行は次の節を引く）：

- 取り違えの数え方、割れた発話、final の無い repeat、向き：10.9.2（T-IP-02-004 から 013）。
- `step` と `lid` の読み口：10.4 の `Transcript` の `fallback`（T-IP-03-001、002、003、025）。
- 10.4 の 3 の境目、SenseVoice の復号の失敗、判定の段が無い時：10.4 の 2 から 5（T-IP-03-006、010、025 から 032）。
- 決めた後の読み直しと SenseVoice の裁き：10.4（T-IP-03-013、014、028）。
- 速報：10.4「速報（draft）は変えない」（T-IP-03-033）。
- 非 live の decode、候補、`session.pending`：10.4 と 10.11 の IP-S3 の条件 3（T-IP-03-017、018、034）。
- 持っただけの候補は event を出さず、`final` を出す所で 1 件出す：10.4（T-IP-03-035 から 039、T-IP-02-016 から 018）。
- 置き場：10.10。
- 偽の訳の server、`?relay=` の読み、scheme と符号化：10.5、10.9 の IP-5、10.11 の 4、10.12 の H39、H40（T-IP-05-001、002、007 から 019）。
- IP-5 の窓（行 7 から 10）、行ごとの判定、`row` の中の `tr` と `of: final` の訳だけを数えること、読む時：10.9.3（T-IP-05-001 から 004）。IP-9（`asr.segment.unread`）の case 行は、test-designer が 10.9.4 と 10.10 から書く。
- `lat-short` の声：10.12 の H20（T-IP-07-002 から 007）。
- IP-4 の `w50` と `w95`：10.9.1 の 3。この節の IP-4 の行は pair − `--lang` の差だけを置く。
- `--interleave` の引数：10.7（T-IP-06-015、016）。
- `stream --own-llama-port`：10.7.1 と IP-8（T-IP-08-001 から 022）。`machine.rs` の `LLAMA_PORT` を引数の値に替える直しは IP-S1 が行い、RP-83 の S83o の引数は `--own-llama-port 18084` を足す（4.75 の RP-83）。

**行が引く設計の節**（この席は受入の条件を足さない）：

- `pipeline.rs` の `mod tests` の置き場と、持った候補（`Plan::Held`）の落ちの印が `Final` まで届くこと：10.10（T-IP-03-036 から 039）。
- `Final.id` の振り方、repeat をまたいで続く `id`、2 つの run の記録を混ぜないこと：10.4（T-IP-02-016 から 018）。
- 実走の test が訳の server を port で選ぶこと、`--port` の読み方を `stream --own-pid` の門と揃えること：10.7.2 と 10.10（T-IP-02-019 から 021）。
- `?relay=` の scheme の大文字小文字、decode した値の前後の空白、`held` の訳の描き方（印と理由を出し、訳の文を出さない）：10.5（T-IP-05-002、012、014、017 から 019）。
- `--own-llama-port` の型と範囲（`0`、`65535`、`65536`、負の数、空の値、先頭に 0 のある引数、引数の重複）：10.7.1（T-IP-08-013、015 から 022）。

### 4.87 BR-1〜BR-40（対面のモードの磨き上げ）

**最新情報**：Chrome DevTools Protocol の CSS domain は、`CSS.getPlatformFontsForNode` が node の子の文字を描いた platform font の使用状況（`familyName`、`postScriptName`、`isCustomFont`、`glyphCount`）を返すと書く（https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/json/browser_protocol.json 、取得 2026-10-23）。

test：受入 test の名前と置き場は設計 10.13.8、10.13.11、10.13.12 の表が決めている。
case の `T-BR-<n>-` の `<n>` は 2 桁で、01 から 40 まである。
case 行の test は、受入 test の置き場（設計の表の最初の file）に置く。例外は次の 16 つである。

- T-BR-13-008 は `native/hayamimi/src/ingest.rs` の `mod tests`。
- BR-5 の `Mailbox` と stream の経路の行（T-BR-05-008 から 018）は `native/hayamimi-relay/src/partial.rs` の `mod tests`、引数の行（020 から 024）は `native/hayamimi-relay/src/cli.rs` の `mod tests`。

- BR-6 の純関数の行は `native/hayamimi/src/log.rs` の `mod tests`、VAD を通す行は `native/hayamimi-asr/src/vad.rs` の `mod tests`。
- BR-4 の T-BR-04-009 と T-BR-04-015 から 022 は BR-3 の run の記録と、その言語ごとの分け方と、`L_tr` の線の判定を読むので `native/hayamimi/tests/br03.rs`。
- T-BR-04-013 と 014 は log の行を組む関数の test なので `native/hayamimi/src/log.rs` の `mod tests`。
- BR-14 と BR-15 の純関数の行は `native/hayamimi-ui/src/model.rs` の `mod tests`、実走の行は `native/hayamimi/tests/br14.rs`。
- BR-19、BR-21、BR-22、BR-23、BR-24、BR-25、BR-30 の純関数の行は `native/hayamimi-ui/src/dashboard.rs` の `mod tests`。
- BR-22 と BR-23 の実走の行は `native/hayamimi/tests/br21.rs`、BR-25 の実走の行は `native/hayamimi/tests/br24.rs`、BR-31 の実走の行は `native/hayamimi/tests/br30.rs`。
- BR-26 と BR-27 の中継の行は `native/hayamimi-relay/tests/br26.rs` と `br27.rs`。
- T-BR-35-004 は `native/hayamimi/src/routes.rs` の `mod tests`。
- BR-36 の旗の行（T-BR-36-011）は `native/hayamimi/src/args.rs` の `mod tests`、BR-3 の run の行と記録を読む行（012 から 015 と 019）は `native/hayamimi/tests/br03.rs`、それ以外（001 から 010、016 から 018）は `native/hayamimi-asr/tests/br36.rs`。
- BR-37 の旗の行（T-BR-37-011）は `native/hayamimi/src/args.rs` の `mod tests`、定数の行（015）は `native/hayamimi-asr/src/partial.rs` の `mod tests`、BR-3 の run の行と記録を読む行（012 から 014）は `native/hayamimi/tests/br03.rs`。
- BR-38 の旗の行（T-BR-38-016）は `native/hayamimi/src/args.rs` の `mod tests`、log の行（017）は `native/hayamimi/src/log.rs` の `mod tests`、言語の表の行（021）は `native/hayamimi/src/strings.rs` の `mod tests`、BR-3 の run の行と記録を読む行（018 から 020）は `native/hayamimi/tests/br03.rs`、それ以外（001 から 015 と 022）は `native/hayamimi-asr/tests/br38.rs`。
- BR-39 の旗の行（T-BR-39-001、002）は `native/hayamimi/src/args.rs` の `mod tests`、言語の表の行（005）は `native/hayamimi/src/strings.rs` の `mod tests`、BR-3 の run の行（006、007）は `native/hayamimi/tests/br03.rs`、それ以外（003、004）は `native/hayamimi-asr/src/pipeline.rs` の `mod tests`。
- BR-40 の旗の行（T-BR-40-001、002）は `native/hayamimi/src/args.rs` の `mod tests`、言語の表の行（003）は `native/hayamimi/src/strings.rs` の `mod tests`、BR-3 の run の行（006、007）は `native/hayamimi/tests/br03.rs`、それ以外（004、005）は `native/hayamimi-asr/src/pipeline.rs` の `mod tests`。
- T-BR-16-024 は合言葉を照らす関数の module の `mod tests`、T-BR-17-015 と 016 は `/send-link` を組む module の `mod tests`（module の名は実装の席が決める）。

どれも作る物である。
行の関数の名前は、受入 test の名前（設計の表）を 001 の行に置き、ほかの行は 1 行 1 本の文にした。
どの test も `fn 名前() -> Result<(), Box<dyn std::error::Error>>` にして `?` で伝播させ、`unwrap`、`expect`、既定への降格、assert の中の整数の幅寄せ、実行時の環境変数の読み取りを書かない。
`#[cfg(test)]` の塊は今在る物の中に足し、消さず、まとめず、動かさない。

入力が「実走：」で始まる行は binary と model か `chrome-headless-shell` を動かし、`#[ignore = "runs the hayamimi binary with the models, which are not in git"]` を付ける。
browser を動かす行は `#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]` を付ける。
入力が「実物：」で始まる行は git の外の `testdata/` か run の記録を読み、`#[ignore = "reads testdata or run records, which are not in git"]` を付ける。
ほかの行は model も `testdata/` も読まず、`--ignored` なしで通る。`--skip` は足さない。

command：cargo は全部 `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 <対象>` の形で打つ。
対象は設計の各 ID の表の `cargo test` の引数のとおりである（BR-26 から BR-31 の表も同じ）。
「実走：」の行は `cargo` の前に `sudo -n nice -n -5` を挟み、後ろに `-- --ignored` を付ける。
「実物：」の行は `sudo` を挟まず、後ろに `-- --ignored` だけを付ける。

「実走：」の行の前に確かめること（取り合う物と、始めてよい条件）は 4.86 の 5 つと同じで、次の 3 つを足す。

- port：8766、8080、8833、8835、18084 を他の process が持っていないこと（`ss -ltn` の出力を貼る）。test が開く `hayamimi`、中継、偽の訳の server、`chrome-headless-shell` の remote debugging の port は、空きを OS に選ばせる。BR-7 の 2 本の 18084 は、10.7.2 の確かめ（`/health` が `ok`）を通してから起こし、止める時は起こした時の `$!` の pid で止める。
- 初めての実行：`target/release` が温まっていない時は、最初の build が長い。run の席は `flock` が取れるまで待ち、待っている間に timeout の値を延ばさない。
- 承認や入力を待って止まる command：`sudo -n` は password を求めず、取れなければ失敗する。失敗したら席は止まって返し、`sudo` なしで再試行しない。

**数え方**：

- **温まった標本と冷えた標本**：run 全体（`--repeat` をまたぐ）で、閉じる `final`（`t_final_us` を持つ）の `lang` ごとに、その言語で最初の閉じる `final` を持つ発話 1 つが冷えた標本である。言語は manifest でなく閉じる `final` の `lang` で決め、`ja` と `en` の外の言語も含める（設計 10.13.13 の 23）。冷えた標本の列は、現れた言語の数だけを持つ。ほかの閉じる `final` を持つ発話は温まった標本である。閉じる `final` を持たない発話はどちらでもなく、数えない。
- **段の値**：`d = t_close − t_end`、`L_dec = final の latency_ms`、`L_tr = t_tr − t_final`、`L_e = t_settled − t_tr`。`t_close = t_final − L_dec`。時刻と長さは整数の µs で持ち、ms は µs を 1000 で割って負の向きへ切り捨てた整数（floor）とする。p50 と p95 は µs の値の nearest-rank を取ってから ms へ切り捨てる（設計 10.13.13 の 1）。特に断らない case は、入力の時刻を 1000 µs の倍数、`latency_ms` を整数で置く。温まった標本が 0 件の時は、p50 と p95 を出さず、記録に「温まった標本 0」と書き、判定の関数は `Err` を返す（設計 10.13.13 の 2）。
- **p50 と p95**：nearest-rank で、n 個の p% は小さい方から `(p × n + 99) / 100`（整数の割り算）番目（4.1）。
- **線の内**：値が線以下。線ちょうどは内、線を 1 ms 超えれば外。
- **区間の数、隣の発話を飲んだ区間、区間の無い発話**（BR-8）：区間は VAD の区間 1 つ。manifest の発話は `[start_sample, end_sample)` の範囲。区間が 2 つ以上の発話の範囲に 1 標本でも掛かれば、その区間は「隣の発話を飲んだ区間」。1 つの発話の範囲に 1 標本でも掛かる区間が 0 個なら「区間の無い発話」。
- **画面の測り**：頁は `chrome-headless-shell` で開き、`Runtime.evaluate` で `getBoundingClientRect`、`getComputedStyle`、`localStorage`、`getAttribute` を読む。pointer は `Input.dispatchMouseEvent`、key は `Input.dispatchKeyEvent`、窓の大きさは `Emulation.setDeviceMetricsOverride`、頁の要求は `Network.requestWillBeSent`、描いた字体は `CSS.getPlatformFontsForNode` で測る。Enter は keyDown で、Space は keyUp で button を押す。許容は位置と幅が ± 2 px、割合が ± 1 ポイント。

**要素の呼び名**（`hayamimi-ui` の今の class と id。B6 の実装が変えない物）：確定文の行 `.row`、言語の印 `.lang-badge`（text が `lang`）、原文 `.row-text`、訳の行 `.tr`、訳の文 `.tr-text`、清書の段落の文 `.refine-text`、Listening の欄 `.strip`、その文 `.strip-text`、確定文の欄の scroll の要素 `#feed`、清書の欄の scroll の要素 `#refined`、2 つの列を持つ要素 `.main`。
境目は `[role="separator"]` で、縦の線は `[aria-orientation="vertical"]`、横の線は `[aria-orientation="horizontal"]`。入れ替えの button は `button[aria-pressed]`。
字体の `select` と倍率の button は、側ごとの `aria-label`（`strings.en.toml` と `strings.ja.toml` の文言。case の test がその file から文言を読んで探す）で名指す。
「上の列」は `.main` の上の段、「清書の欄」は `#refined` を含む `section` である。
設計が名を決めていない要素（区切りの行）は、行の入力に探し方を書く。

**合否と記録**：

- 合否に置く行は、設計 10.13.7 の合否の ID（BR-1、BR-2、BR-3、BR-4、BR-5、BR-6、BR-11、BR-13、BR-14、BR-15、BR-16、BR-17、BR-35、BR-36）と、BR-19 から BR-34 の全行である（BR-26 から BR-31 の合否の置き方は設計 10.13.12 の slice の表が決める）。
- BR-7、BR-8、BR-9、BR-10、BR-12、BR-18 の実走の数は記録だけで、行の合否は、数える関数が既知の入力を正しく数えることだけに置く。実走の行は数を記録の file に書き、線の外でも落とさない。

**材料**：

- BR-1 の記録の fixture は、test が `env!("CARGO_TARGET_TMPDIR")` の下に作る合成の `.jsonl`（1 行が `Sample`）と `.events.jsonl`（1 行が `{"event":{…},"t_us":<n>}`）である（前例は T-IP-04 の道具の行）。
- 記録 A（3 発話。`ja`、`en`、`ja`）：`session_start` を `t_us` 9000000 に置く。発話 1（`ja`）は `t_start_us` 9500000、`t_end_us` 12000000。final は `t_us` 12745000、`latency_ms` 268、`t1_s` 3.0、`id` が `file-1`、`lang` が `ja`。草稿の訳（`of: final`、`settled: false`、`status: ok`、`src_sha256` は final の text の sha256）が `t_us` 12978000、確定の訳（`settled: true`）が 13700000。発話 2（`en`）は `t_start_us` 14000000、`t_end_us` 16000000。final は `t_us` 16300000、`latency_ms` 200、`t1_s` 7.0、`id` が `file-2`。草稿の訳が 16350000、確定の訳が 16900000。発話 3（`ja`）は `t_start_us` 17000000、`t_end_us` 19000000。final は `t_us` 19550000、`latency_ms` 250、`t1_s` 10.0、`id` が `file-3`。草稿の訳が 19650000、確定の訳が 20150000。
- 記録 B（7 発話）：発話 1 と 2 は記録 A と同じ。発話 3 から 7 は温まった標本で、`(d, L_dec, L_tr)` を ms で `(250, 200, 50)`、`(280, 250, 60)`、`(300, 270, 100)`、`(450, 300, 150)`、`(600, 480, 300)` とする。各発話は、終わりを T として、final が T + d + L_dec、草稿の訳が final + L_tr、確定の訳が草稿 + 500 ms に着く。`t1_s` は T から 9 を引いた秒。言語は `ja`、`en`、`ja`、`en`、`ja` の順。
- 予算（設計 10.13.3）：d は 300 と 600、L_dec は 270 と 480、L_tr は 100 と 300、`L_pg` は 50 と 100（p50 と p95 の順）。主指標の線は `L_page` が 750 と 1300、`L_settled_page` が 1100 と 1700、冷えた標本の最大は `L_page` 2500。
- BR-11、BR-13 の実物は `docs/results/in-person/ip04_pair.jsonl` と `ip04_pair.events.jsonl` である（90 行。`jq` で 2026-10-23 に数えた：`t_final_us` が null の行は 12、そのうち utt 3 は `finals` 0、utt 9 は 1、utt 19 は 2、utt 21 は 1、各 3 repeat）。
- BR-9、BR-10 の test は、`docs/results/in-person/ip02_pair.*`、`ip02_fast.*`、`ip07_pair.*` と、同じ directory の `ip02.md`、`ip07.md` の記録の行を、手で数えた時点で複写して読む。複写を取るのは、その test を書く席である。複写の名前は読む test の step を頭に置く（`br09_ip7_pair.*`、`br09_ip7.md`、`br10_ip2_pair.*`、`br10_ip2_fast.*`、`br10_ip7_pair.*`、`br10_ip2.md`、`br10_ip7.md`）。
- 実走が書き直す記録の名前（`ip02_pair.*`、`ip02_fast.*`、`ip07_pair.*`、`br03_ip*`）を書いてよい test の file は、その実走を持つ `ip02.rs`、`ip07.rs`、`br03.rs` だけである（T-BR-03-019）。手で数えた値を当てる test は、どの実走も書かない名前の複写を読む（`ip04_pair.*` は T-BR-03-002、BR-11、IP-4 の test が、`br04_ip_s8.*` は T-BR-04-009 が読む。複写の名前は読む test の step を頭に置き、書き直される記録の名前を部分文字列として含めない。T-BR-03-019 が部分文字列で数えるので、含めると複写を読む test の file が止まる。複写は手で数えた時点の記録と同じ bytes である）。
- 画面の行（BR-14、BR-15、BR-19 から BR-25、BR-30、BR-31）の「実走」は、設計 10.13.11 の受入 ID の前置きと同じく、偽の訳の server（`fake_llama_server -m translates.gguf`）で `lat-dialog` の行 7 から 10（`ja`、`en`、`ja`、`en`）を流した頁を測る。起動の形は T-IP-05-001 と同じである。

**BR-1（段ごとの遅れの記録。`native/hayamimi-eval/tests/br01.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-01-001 | 受入：記録 A を記録の関数に渡す | 3 発話。発話 1 と 2 は自分の言語で最初の閉じる final を持ち、発話 3 は温まった標本 | 段の値が ms まで一致 | 発話 1 は `d` 477、`L_dec` 268、`L_tr` 233、`L_e` 722。発話 2 は `d` 100、`L_dec` 200、`L_tr` 50、`L_e` 550。発話 3 は `d` 300、`L_dec` 250、`L_tr` 100、`L_e` 500。発話 1 と 2 が冷えた標本の列、発話 3 が温まった標本の列（n は 1）に入る。発話ごとに `d + L_dec + L_tr` が `t_tr − t_end`（3.1 の `L`）に等しい（978、350、650）。温まった標本の p50 と p95 は d が 300 と 300、L_dec が 250 と 250、L_tr が 100 と 100 で、4 つの判定がすべて「線の内」（specified：設計 10.13.3 の時刻の表と BR-1。derived：手計算。`session_start` の 9 秒が無いと final が窓の外に置かれ、この値は出ない） | 陽性 | `br01_each_stage_of_the_wait_is_recorded_on_one_clock` |
| T-BR-01-002 | 記録 B | 温まった標本 5 件。各段の p50 と p95 が線ちょうど | d 300 と 600、L_dec 270 と 480、L_tr 100 と 300 | 温まった標本の n は 5、冷えた標本は 2 件。d の p50 は 300、p95 は 600、L_dec は 270 と 480、L_tr は 100 と 300。6 つの判定がすべて「線の内」（specified：設計 10.13.3 と本節の数え方。nearest-rank の 3 番目と 5 番目。derived：手計算） | 陽性、境界 | `a_run_whose_stages_stand_on_their_lines_is_written_inside_the_lines` |
| T-BR-01-003 | 記録 B の発話 5 の d を 301 に替える（温まった標本の d の昇順は 250、280、301、450、600） | d の p50 が線の 1 つ外 | d の p50 300 | d の p50 は 301 で「線の外」、d の p95（600）は「線の内」。ほかの段は「線の内」（derived：手計算。陰性対照：300 と 301 の差を見分ける） | 境界、陰性 | `a_d_p50_of_301_ms_is_outside_its_line_while_p95_stays_inside` |
| T-BR-01-004 | 記録 B の発話 7 の d を 601 に替える | d の p95 が線の 1 つ外 | d の p95 600 | d の p95 は 601 で「線の外」、p50（300）は「線の内」（derived。003 と対で、p50 と p95 を取り違える実装を落とす） | 境界、陰性 | `a_d_p95_of_601_ms_is_outside_its_line_while_p50_stays_inside` |
| T-BR-01-005 | 記録 B の発話 5 の d を 299、発話 7 の d を 599 に替える | 線の 1 つ内側 | d の線 | d の p50 は 299、p95 は 599、どちらも「線の内」（derived：003 と 004 の反対側の境界） | 境界 | `a_d_p50_of_299_ms_and_p95_of_599_ms_are_inside_their_lines` |
| T-BR-01-006 | 記録 B の発話 5 の L_dec を 271 に替える | L_dec の p50 が 1 つ外 | L_dec の p50 270 | L_dec の p50 は 271 で「線の外」、p95（480）は「線の内」（derived） | 境界、陰性 | `a_decode_p50_of_271_ms_is_outside_its_line` |
| T-BR-01-007 | 記録 B の発話 7 の L_dec を 481 に替える | L_dec の p95 が 1 つ外 | L_dec の p95 480 | L_dec の p95 は 481 で「線の外」、p50（270）は「線の内」（derived） | 境界、陰性 | `a_decode_p95_of_481_ms_is_outside_its_line` |
| T-BR-01-008 | 記録 B の発話 5 の L_tr を 101 に替える | L_tr の p50 が 1 つ外 | L_tr の p50 100 | L_tr の p50 は 101 で「線の外」、p95（300）は「線の内」（derived） | 境界、陰性 | `a_draft_translation_p50_of_101_ms_is_outside_its_line` |
| T-BR-01-009 | 記録 B の発話 7 の L_tr を 301 に替える | L_tr の p95 が 1 つ外 | L_tr の p95 300 | L_tr の p95 は 301 で「線の外」、p50（100）は「線の内」（derived） | 境界、陰性 | `a_draft_translation_p95_of_301_ms_is_outside_its_line` |
| T-BR-01-010 | 記録 B に、閉じる final の無い発話（final を終わりの前に置く）を 1 つ足す | 閉じる final が無い | 数えない | その発話は段の値を持たず、温まった標本の n は 5 のまま、冷えた標本は 2 件のまま、各段の p50 と p95 は 002 と同じ（specified：BR-1 の陰性対照。002 が数える式の陽性対照） | 陰性 | `an_utterance_without_a_closing_final_has_no_stage_values_and_is_not_counted` |
| T-BR-01-011 | `(d, L_dec, L_tr)` の各軸に 0、1、499、500、501、2000 ms を取った 216 通りの格子。各点で 1 発話の記録を組む | 0 と 1 の境、桁の境 | 3 段の和が `L` | どの点でも `d + L_dec + L_tr` が `t_tr − t_end` に ms まで等しい（property：式は 3.1 の `L` の定義で、記録の関数の外で組む。specified：BR-1 の陽性対照） | property | `the_three_stages_add_up_to_the_wait_of_3_1_at_every_grid_point` |
| T-BR-01-012 | 記録 B の発話 3 の草稿の訳を、final より 40 ms 前（速報の訳の使い回し）に置く（`L_tr` が −40） | 負の `L_tr` | 丸めない | 発話 3 の `L_tr` は −40、`d + L_dec + L_tr` は 250 + 200 − 40 = 410 で `L` に等しい。温まった標本の L_tr の昇順は −40、60、100、150、300 で、p50 は 100、p95 は 300（derived：`Sample::l_tr_us` の doc「Negative when a partial's translation matched」。0 に丸めない） | 境界 | `a_draft_translation_that_reused_a_partial_keeps_its_negative_wait` |
| T-BR-01-013 | 記録 B の発話 1 から閉じる final を取り（発話 1 は final なし）、発話 3 が `ja` の最初の閉じる final になる | 冷えた標本の選び方 | `ja` で最初の閉じる final | 冷えた標本は発話 2（`en`）と発話 3（`ja`）で、発話 1 はどちらでもない。温まった標本は発話 4 から 7 の 4 件（specified：設計 10.13.3 と 10.13.13 の 23「言語ごとに最初の閉じる final を持つ発話」） | 境界 | `the_cold_sample_of_a_language_is_the_first_utterance_that_has_a_closing_final` |
| T-BR-01-020 | 記録 B の発話 4（`en`）の final の `lang` を `zh` に替える（`ja` の発話 1、`en` の発話 2 は閉じる） | `ja` と `en` の外の言語 | 言語ごとに最初の 1 件 | 冷えた標本は発話 1（`ja`）、発話 2（`en`）、発話 4（`zh`）の 3 件で、冷えた標本の列が 3 件。温まった標本は発話 3、5、6、7 の 4 件。発話 6（`en`）は温まった標本（specified：設計 10.13.13 の 23「`ja` と `en` の外の言語も含め」「現れた言語の数だけ」） | 境界 | `a_language_outside_ja_and_en_also_gets_its_own_cold_sample` |
| T-BR-01-014 | 記録 B を `repeat` 1 とし、同じ 7 発話を `repeat` 2 として続ける（`session_start` は 1 回だけ） | 同じ process の 2 回目 | 冷えた標本は言語ごとに 1 件 | 冷えた標本は 2 件（repeat 1 の発話 1 と 2）で、repeat 2 の発話 1 と 2 は温まった標本。温まった標本の n は 5 + 7 = 12（specified：設計 10.13.3「stream --repeat 3 の 2 回目と 3 回目は冷えた標本を持たない」） | 境界 | `a_second_repeat_in_the_same_process_has_no_cold_sample` |
| T-BR-01-015 | 記録 B の発話 4 の final から `latency_ms` を取る | Python の server の形（`latency_ms` が無い） | `t_close` が置けない | 発話 4 は `d` と `L_dec` を持たず、`L_tr` は持つ。d と L_dec の n は 4、L_tr の n は 5（derived：`t_close = t_final − L_dec` が `L_dec` を要る。0 を埋めない） | 境界 | `a_closing_final_without_latency_ms_has_no_d_and_no_decode_but_keeps_its_translation_wait` |
| T-BR-01-016 | 記録 A の発話 1 と 2 だけの記録（冷えた標本の 2 発話だけ）を記録の関数に渡す | 温まった標本が 0 件 | 0 件を成果と読まない | 記録に「温まった標本 0」と書き、温まった標本の p50 と p95 を出さず、「線の内」とも「線の外」とも書かない。判定の関数が `Err` を返す。冷えた標本の列には 2 件ある（specified：設計 10.13.13 の 2 と BR-1 の陰性対照。implicit：`nearest_rank` は空で `Err`） | 陰性 | `a_run_with_no_warm_sample_is_not_written_as_inside_the_lines` |
| T-BR-01-017 | 記録 B の発話 3 の確定の訳を、(a) 無し、(b) `status: held`、(c) `status: ok` で置く | `L_e` の有無 | 記録だけ | (c) は `L_e` 500。(a) と (b) は `L_e` を持たない。`L_e` の p50 と p95 は書かれ、判定の語（線の内、線の外）は付かない（specified：設計 10.13.3「確定の訳：記録だけ」） | 境界 | `the_settle_wait_is_recorded_without_a_verdict_and_needs_an_ok_settled_translation` |
| T-BR-01-018 | 標本が 0 行の `.jsonl` | 空の記録 | 空は誤り | `Err` で、記録を書かない（implicit：0 件を「全部内」と読まない。001 から 017 が非空の陽性対照） | 陰性 | `a_record_with_no_line_is_an_error` |
| T-BR-01-019 | 3 つの記録。(a) `t_end_us` 12000000、final の `t_us` 12745000 と `latency_ms` 268.6、草稿の訳が final の 233000 µs 後。(b) 草稿の訳が final の 400 µs 前の発話。(c) `t_end_us` 12000000、final の `t_us` 12745800 と `latency_ms` 268.9、草稿の訳が final の 233900 µs 後 | µs が ms の整数に収まらない | floor | (a) は `L_dec` の µs が 268600（`latency_ms` に 1000 を掛けて切り捨て）、`t_close` が 12476400 なので `d` 476.4 → 476、`L_dec` 268、`L_tr` 233、`L` 978。µs の和 476400 + 268600 + 233000 が `L` の 978000 に完全に等しく、ms の和 977 は `L` と 1 ms 違う。(b) の `L_tr` は −1（−0.4 を負の向きへ切り捨て。0 でない）。(c) は `d` 476、`L_dec` 268、`L_tr` 233、`L` 979 で、ms の和 977 が `L` と 2 ms 違い（許される上限）、µs の和は完全に一致する。記録の ms の表は段ごとの切り捨てのまま書かれ、和を合わせる足し引きが無い（specified：設計 10.13.13 の 1） | 境界 | `stage_values_are_floored_to_milliseconds_and_negative_ones_floor_away_from_zero` |

**BR-2（頁の段 `L_pg`。`native/hayamimi/tests/br02.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-02-001 | 受入、実走：`<hy> --input ws --mode pair --source file --serve <port>`、偽の訳の server、`<relay> live` を起こし、`/dashboard?relay=…` を開いて測りの script を `Runtime.evaluate` で入れ、`lat-dialog` の行 7 から 10 を流す | 4 行。照合が止めた行（数字を持つ行 9 など）は訳の代わりに印を描き、数に入らない | 表示された訳 1 件につき `p_arrive` と `p_draw` が 1 つ | 表示された訳が 1 件以上で、そのそれぞれに `p_arrive` と `p_draw` が 1 つずつ在り、`L_pg = p_draw − p_arrive` が 0 以上。表示された訳の数と印の数の和が頁の `row` の数に等しく、`row` は発話の数の 4 以上である。記録に p50 と p95 が書かれる（specified：BR-2。差は頁の `performance.now()` の 2 つの値だけで取る。derived：中継は数字を持つ元の行の訳を止めるので、4 件を固定すると窓の行の読みで落ちる） | 陽性 | `br02_the_page_stage_is_timed_on_the_page_clock` |
| T-BR-02-002 | 001 の run の `p_draw` の数と、同じ頁の `row` の中で `tr-text` を持つ `tr` の数 | 測りの script が数える物 | 2 つの数が等しい | `p_draw` の数 = `row` の中で訳の文を持つ `tr`（`tr-text` を持つ物）の数 > 0（specified：BR-2 の陽性対照。script が描きを取りこぼしていない。印だけの `tr` は描きに数えない） | 陽性 | `the_measuring_script_sees_as_many_draws_as_the_page_has_translation_rows` |
| T-BR-02-003 | 同じ run の間に `?relay=` の無い `/dashboard` を開き、同じ script を入れる | 訳が出ない頁 | `p_draw` が 0 件 | `row` が発話の数の 4 以上で、`row` の中の `tr-text` を持つ `tr` と印だけの `tr` がどちらも 0。`p_draw` が 0 件で、記録の関数が `Err`（specified：BR-2 の陰性対照。0 件の p50 を書かない） | 陰性 | `a_page_without_relay_in_its_address_gives_no_draw_and_an_error` |
| T-BR-02-004 | 道具：頁の時計の `(p_arrive, p_draw)` を `(100.0, 140.0)`、`(200.0, 200.0)`、`(300.0, 299.0)` で記録の関数へ | `L_pg` が 40、0、−1 ms | 0 以上 | `L_pg` が 0 以上という述語が、1 つ目と 2 つ目を通し、3 つ目で `Err`（specified：BR-2。0 ちょうどは内、−1 は外の両側） | 境界 | `a_draw_before_its_arrival_on_the_page_clock_is_an_error` |
| T-BR-02-005 | 道具：訳 4 件のうち 1 件の `p_draw` が無い記録 | 描きの取りこぼし | 1 つずつ | `Err` で、`p_draw` の無い訳を名指す（specified：BR-2「のそれぞれに …1 つずつ」） | 陰性 | `a_translation_without_a_draw_record_is_named_and_is_an_error` |
| T-BR-02-006 | 道具：`L_pg` が 10、20、30、40 の 4 件 | nearest-rank の n = 4 | p50 は 2 番目、p95 は 4 番目 | p50 が 20、p95 が 40（derived：本節の nearest-rank。4.1） | 陽性 | `the_page_stage_percentiles_are_nearest_rank` |
| T-BR-02-007 | 道具：同じ `L_pg` の組に、道具の時計の時刻（`t_tr` ほか）へ 1 秒を足した物を添えて 2 回渡す | 道具の時計の値 | 頁の段は頁の時計だけ | 2 回の `L_pg` の列が等しい（property：頁の段が道具の時計に依らない。specified：設計 10.13.3「頁の段は、頁の時計の 2 つの時刻の差だけで取る」） | property | `the_page_stage_does_not_depend_on_the_tool_clock` |
| T-BR-02-008 | 道具と実物：`rg -n 'p_arrive\|p_draw' native/hayamimi-ui/src native/hayamimi/data/pages`、陽性対照として同じ式を `native/hayamimi/tests/br02.rs` に | 製品の頁の code に測りの口が無い | 0 件 | 製品側 0 件（rg の rc 1）、test 側 1 件以上（specified：設計 10.13.3「製品の頁の code には測りの口を足さない」。0 件は陽性対照の非 0 が付いて初めて読む） | 陽性、陰性 | `no_product_page_code_carries_the_measuring_hook` |

**BR-3（予算の線。`native/hayamimi/tests/br03.rs`）**

材料：実走は、段ごとの値を `docs/results/in-person/<step>.md` に、標本と events を同じ名前の `.jsonl` と `.events.jsonl` に、run の log のうち速さの手が読む event（`asr.vad.closed`、`asr.partial.lang.asked`、`translate.slot.waited`、`translate.partial.skipped`、`translate.draft.reused`）の行を `.log.jsonl` に残す。`translate.draft.reused` は確定が速報の訳を使い回した行で、言語は同じ記録の `.events.jsonl` の確定の行の `id` から引く（設計 10.13.10 の H51）。`<step>` は手ごとに変え（IP-S7 と IP-S8 を入れた形は `br03_ip_s8`、旗で IP-S8 を外した対照は `br03_ip_s8_lid_only`、2 つとも外した対照は `br03_ip_s8_both_off`）、前の手の記録を上書きしない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-03-001 | 受入、実走：10.7.2 の 18084 の本物の訳の server で `lat-dialog` を `--repeat 3` で流し、BR-1 と BR-2 の関数を当てる。B2 の段を全部入れた後にだけ合否を出す | 温まった標本と冷えた標本 | `L_page` 750 と 1300、`L_settled_page` 1100 と 1700、冷えた最大 2500 | 温まった標本で `L_page` の p50 が 750 以下、p95 が 1300 以下、`L_settled_page` の p50 が 1100 以下、p95 が 1700 以下。冷えた標本の最大が `L_page` 2500 以下。段ごとの予算の内外も記録に書く。記録を書いた後に、T-BR-04-009 の判定（`L_tr` の p50 が 100 ms 以下）も当てる（specified：BR-3、BR-4） | 陽性 | `br03_the_wait_from_the_end_of_speech_to_the_page_is_inside_the_budget` |
| T-BR-03-002 | 実物：`ip04_pair.jsonl` と `ip04_pair.events.jsonl` に BR-1 の関数を当てる | 冷えた標本 2 件（`repeat` 1 の utt 1 と utt 2）を除いた 76 件 | 設計 10.13.2 の pair の「温まった標本」の行 | 温まった標本の n が 76、d の p50 と p95 が 445 と 1081、L_dec が 268 と 472、L_tr が 221 と 363。冷えた標本の列が 2 件で、repeat 1 の utt 1 と utt 2（specified：設計 10.13.2 の表と BR-3 の陽性対照。derived：別に組んだ `jq` でも 2026-10-23 に同じ値を数えた） | 陽性 | `the_warm_stage_percentiles_of_the_pair_record_are_the_hand_counted_ones` |
| T-BR-03-003 | 道具：温まった標本 20 件がすべて `L_page` 750 ms、冷えた標本 2 件は 1000 ms | p50 が線ちょうど | 750 | 判定の関数が `Ok`（specified：BR-3。p95 750 も線の内） | 境界 | `a_page_wait_p50_of_750_ms_is_on_the_line` |
| T-BR-03-004 | 003 の 20 件を 751 ms に替える | p50 が 1 つ外 | 750 | `Err` で、「`L_page` の p50 が 751 ms で線 750 ms の外」と言う（specified：BR-3 の陰性対照。1 ms 超えた記録） | 境界、陰性 | `a_page_wait_p50_of_751_ms_is_over_the_line` |
| T-BR-03-005 | 20 件のうち 18 件が 700 ms、残り 2 件が 1300 ms | p95 が線ちょうど（20 件の p95 は 19 番目） | 1300 | `Ok`（specified：BR-3。nearest-rank） | 境界 | `a_page_wait_p95_of_1300_ms_is_on_the_line` |
| T-BR-03-006 | 005 の 2 件を 1301 ms に替える | p95 が 1 つ外 | 1300 | `Err`（p95 の外と言う）（specified：BR-3 の陰性対照） | 境界、陰性 | `a_page_wait_p95_of_1301_ms_is_over_the_line` |
| T-BR-03-007 | 18 件が 700 ms、19 番目が 1300 ms、20 番目が 5000 ms | 最大の 1 件 | p95 は最大で決まらない | `Ok`（derived：4.1 と T-IP-04 の「最大の clip は p95 を決めない」と同じ式） | 境界 | `the_largest_sample_does_not_decide_the_page_wait_p95` |
| T-BR-03-008 | 温まった標本 20 件がすべて `L_settled_page` 1100 ms | p50 が線ちょうど | 1100 | `Ok` | 境界 | `a_settled_page_wait_p50_of_1100_ms_is_on_the_line` |
| T-BR-03-009 | 008 を 1101 ms に | p50 が 1 つ外 | 1100 | `Err` | 境界、陰性 | `a_settled_page_wait_p50_of_1101_ms_is_over_the_line` |
| T-BR-03-010 | 18 件が 1000 ms、残り 2 件が 1700 ms | p95 が線ちょうど | 1700 | `Ok` | 境界 | `a_settled_page_wait_p95_of_1700_ms_is_on_the_line` |
| T-BR-03-011 | 010 の 2 件を 1701 ms に | p95 が 1 つ外 | 1700 | `Err` | 境界、陰性 | `a_settled_page_wait_p95_of_1701_ms_is_over_the_line` |
| T-BR-03-012 | 温まった標本 20 件が 700 ms、冷えた標本が 2500 ms と 100 ms | 冷えた最大が線ちょうど | 2500 | `Ok` | 境界 | `a_cold_sample_of_2500_ms_is_on_the_line` |
| T-BR-03-013 | 012 の 2500 を 2501 に | 冷えた最大が 1 つ外 | 2500 | `Err`（冷えた標本の線の外と言う） | 境界、陰性 | `a_cold_sample_of_2501_ms_is_over_the_line` |
| T-BR-03-014 | 温まった標本 20 件が 700 ms、冷えた標本が 2400 ms の 2 件（2400・100 の 1 件ずつでも `Ok`） | 冷えた標本の分離 | p95 に冷えた標本を入れない | `Ok`（混ぜると 22 件の 21 番目が 2400 で p95 の線 1300 の外になる。derived：手計算。分けた実装だけが通る） | 陰性 | `a_cold_sample_is_kept_out_of_the_warm_percentiles` |
| T-BR-03-015 | 温まった標本の d の p50 だけが 301 ms で、`L_page` の 4 つの線は内 | 段の予算の外と主指標の内 | 段の外は記録 | 判定の関数は `Ok`、記録は d の p50 を「線の外」と書く（specified：BR-3「段ごとの予算の内外も記録する」。段の外では落とさない） | 境界 | `a_stage_outside_its_budget_is_recorded_and_does_not_fail_the_run` |
| T-BR-03-016 | 温まった標本が 0 件の記録 | 空 | 0 件を成果と読まない | `Err`（implicit：T-BR-01-016 と同じ読み） | 陰性 | `a_run_with_no_warm_sample_cannot_pass_the_budget` |
| T-BR-03-017 | 実走：001 と同じ run を、`hayamimi-relay live` に `--partial-yield off` を付けて起こす（IP-S8 を外し、IP-S7 は既定の on） | IP-S8 の寄与を分ける | 記録だけ | 記録 `br03_ip_s8_lid_only.{md,jsonl,events.jsonl,log.jsonl}` が書かれる。合否は付けない（specified：設計 10.13.9 の IP-S8「記録：BR-3 の run の段ごとの値」。旗を外した run と既定の run を同じ機械で並べる） | 陽性 | `the_run_with_the_final_translation_not_yielding_is_recorded` |
| T-BR-03-018 | 実走：001 と同じ run を、`hayamimi` に `--partial-lid off`、`hayamimi-relay live` に `--partial-yield off` を付けて起こす（IP-S7 と IP-S8 を外す） | IP-S7 と IP-S8 を入れる前の形 | 記録だけ | 記録 `br03_ip_s8_both_off.{md,jsonl,events.jsonl,log.jsonl}` が書かれる。合否は付けない（specified：設計 10.13.9 の IP-S7「記録：BR-3 の run の段ごとの値」。入れる前の値を同じ機械で取る） | 陽性 | `the_run_with_both_measures_switched_off_is_recorded` |
| T-BR-03-019 | 検査：`native/` の下の Rust の全 file の本文に、実走が書き直す記録の名前（`ip02_pair`、`ip02_fast`、`ip07_pair`、`br03_ip`）が在るかを数える | 4 つの名前（本文に部分文字列として在るかを数える）と、書いてよい file（`ip02.rs`、`ip02.rs`、`ip07.rs`、`br03.rs`）。`target` で始まる directory は走査しない | 手で数えた値を当てる test が、実走で書き直される記録を読まない | 書いてよい file の外で名前を書いた file が 0 件。走査した file が 200 より多い（`find native -name '*.rs'` から `target` 以下を除いて 2026-10-27 に数えた 444）。名前を含む合成の本文は、書いてよい file の名前では見つからず、それ以外の名前では 4 つとも見つかる（陽性対照） | 陰性 | `only_the_owner_of_a_run_names_the_records_that_run_rewrites` |

**BR-4（速報の言語を発話の途中で決める。`native/hayamimi-asr/tests/br04.rs`）**

材料：偽の判定器は入力の標本数で答える。区間の全体と同じ長さの入力（確定の側の判定）には「確定の答え」を、32000 標本以上でそれより短い入力（途中の判定）には「途中の答え」を、それより短い入力（直前の確定文を作る 0.3 秒の読み）には「最初の答え」を返し、呼ばれた入力の標本数を全部覚える。「途中の答え」と「確定の答え」は失敗にもできる。偽の `Decode` は、速報の decode が呼ばれた tier と読んだ本文、`prefetch` が呼ばれた tier を順に覚える。区間は 512 標本（`WINDOW`）の窓で 1 窓ずつ流す。直前の確定文の言語は、先に 0.3 秒の区間を 1 件流して作る（最初の答え）。前例は `ip03.rs` の `Heard` と `Models` である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-04-001 | 受入：直前の確定文が `ja`。途中の答え `en`、確定の答え `en`。声が 125 窓（64000 標本）続く区間を流す | 途中の判定より後に速報の decode が 1 回以上ある | 2.0 秒 | 途中の判定より前の速報の decode はすべて ja の tier、後の速報の decode はすべて en の tier で、後の呼びは 1 回以上。後の速報の本文は en の tier の読み（英字）で、前の速報の本文は ja の tier の読み（仮名）。中継は速報の本文の文字から `src_lang` を読む（`partial_language`）ので、後の速報の訳の `src_lang` は `en` になる。途中の判定の後で `prefetch` が呼ばれた tier は `V3` の 1 件。確定文の `lang` は `en`。判定器の呼びは 2 回（途中の 1 回と区間の全体の 1 回）（specified：BR-4。後の呼びが 0 回の run を通と読まないために 1 回以上を求める） | 陽性 | `br04_a_partial_after_a_switch_is_read_in_the_language_heard_so_far` |
| T-BR-04-002 | 001 と同じ入力で `--partial-lid off` | 旗を切る | 今の形 | 速報の decode はすべて ja の tier で本文は仮名（速報の訳の `src_lang` は `ja`）、途中の判定の呼びは 0 回（判定器が覚えた入力は区間の全体の 1 つだけ）、`prefetch` の呼びは 0 回（specified：BR-4 の陽性対照） | 陽性 | `with_partial_lid_off_the_partial_stays_in_the_language_of_the_last_final` |
| T-BR-04-003 | 直前の確定文が `ja`。声が 62 窓（31744 標本、1.984 秒）の区間 | 2.0 秒の 1 窓下 | 32000 標本 | 判定器の呼びは 1 回（区間の全体の入力 31744 標本）で、途中の判定は呼ばれない（specified：設計 10.13.4 の手 1 と 10.13.13 の 20「声の標本が初めて 32000 以上になった窓の直後」。62 窓は 31744 で 32000 に届かない：境界の下側） | 境界 | `a_segment_of_62_windows_gets_no_midway_identification` |
| T-BR-04-004 | 003 の声を 63 窓（32256 標本、2.016 秒）にする | 2.0 秒を超える最初の窓 | 32000 標本 | 判定器の呼びは 2 回で、1 回目の入力が 32256 標本（63 窓を渡した直後。32000 から 32511 の間で、32000 ちょうどに切らない）、2 回目が区間の全体。声が 64、72、80、90、100、110、120、125 窓の 8 通りの区間でも、1 回目の入力はいつも 32256 標本で、判定器の呼びは 2 回（specified：設計 10.13.13 の 20 と 10.13.4 の「1 発話に 1 回だけ足す」） | 境界 | `a_segment_of_63_windows_gets_one_midway_identification` |
| T-BR-04-005 | 直前の確定文が `en`。途中の答え `zh`、確定の答え `en`。声が 125 窓 | 途中の答えが 2 つの外 | 外なら今のまま | 速報の decode はすべて en の tier で本文は英字（速報の訳の `src_lang` は `en`）、途中の判定の呼びは 1 回、`zh` の tier の decode も `prefetch` も呼ばれない（specified：BR-4 の陰性対照「判定器が zh を返す区間」。pair は 2 つの外の言語を読まないので、読み込むと常駐の枠を使うだけになる） | 陰性 | `a_midway_answer_outside_the_pair_leaves_the_partial_where_it_was` |
| T-BR-04-006 | 直前の確定文が `ja`。1 つ目の区間は途中の答え `en`、確定の答え `ja`（125 窓）。続けて 2 つ目の区間（40 窓、1.25 秒）を流す | 途中の答えが次の発話へ漏れない | 確定文の言語を変えない | 2 つ目の区間の速報の decode はすべて ja の tier（直前の確定文の言語）で、en の tier は 1 回も呼ばれない（specified：設計 10.13.4「2.0 秒より前の速報は今のとおり直前の確定文の言語で読む」と「確定文の言語の決め方を変えない」） | 陰性、境界 | `the_midway_answer_does_not_become_the_language_of_the_next_segment` |
| T-BR-04-007 | 006 の 1 つ目の区間を、`--partial-lid on` と `off` のそれぞれで流す | 確定の側の判定 | 呼ばれる回数も答えも変えない | 区間の全体の入力での判定器の呼びが両方で 1 回ずつ。確定文の `lang` が両方で `ja`、`fallback`（IP-3 の降り方）が両方で `None`。途中の判定は on の run にだけ 1 回（specified：BR-4「確定文の言語の決め方（10.4）は呼ばれる回数も答えも変わらない」） | 陽性、property | `the_final_identification_is_called_and_answers_the_same_with_the_midway_one_on_or_off` |
| T-BR-04-008 | `-p hayamimi-asr --test ip03` を、この変更を入れた build で打つ | 既存の受入 | 全部通る | T-IP-03 の全行が変更前と同じに通る（specified：BR-4 の陰性対照「IP-3 の test がそのまま通る」。受入 test の名前は `ip03_an_unsure_identifier_falls_to_ja_or_en`） | 陰性 | `ip03_an_unsure_identifier_falls_to_ja_or_en` |
| T-BR-04-009 | 実走の記録を読む：IP-S7 と IP-S8 を既定で載せた BR-3 の run の記録の複写 `docs/results/in-person/br04_ip_s8.{jsonl,events.jsonl}`（`native/hayamimi/tests/br03.rs` に置く。元の `br03_ip_s8.*` は BR-3 の既定の実走が書き直すので、手で数えた値を当てるこの行は読まない） | pair の温まった標本の `L_tr` | p50 100 ms | `L_tr` の p50 が 100 ms 以下で、`ja` と `en` のどちらにも温まった値が在る。言語ごと（`ja` の行と `en` の行）の n と p50 と p95 は、記録から手で数えた値と等しい（specified：BR-4 と設計 10.13.10 の H51。分けた列の n が 0 なら、その列を通と読まない）。同じ判定を 001 の実走の終わりにも当てる | 陽性 | `the_warm_pair_run_has_its_draft_translation_p50_inside_100_ms_and_split_by_language` |
| T-BR-04-010 | 直前の確定文が `ja`。途中の答えは失敗、確定の答え `en`。声が 125 窓 | 途中の判定の失敗 | 失敗は覚えて、同じ発話では聞き直さない | 速報の decode はすべて ja の tier、途中の判定の呼びは 1 回（32256 標本）で、`prefetch` は 0 回。確定文の `lang` は `en` で、判定器の呼びは合わせて 2 回（specified：設計 10.13.4 の手 1「確定の側の判定は変えない」。失敗は確定文の言語を動かさない） | 陰性、境界 | `a_failed_midway_question_is_kept_and_not_asked_again` |
| T-BR-04-011 | pair でない 4 通りの policy（balanced、fast、`--lang ja`、`partial_lid` が on のまま pair の旗だけを切った policy）で、声が 125 窓の区間 | pair の外 | 聞かない | 判定器の呼びは 0 回、速報の decode はすべて ja の tier（先に確定文が無いので ja の tier が読む）（specified：設計 10.13.4 の手 1「pair だけ」） | 陰性 | `a_run_that_is_not_a_pair_run_never_asks_the_midway_question` |
| T-BR-04-012 | 途中の答えが `en`、`zh`、失敗の 3 通りで、声が 125 窓の区間。続けて声が 62 窓の区間 | 途中の判定が出す記録 | 1 発話に 1 件 | 3 通りとも、途中の判定の記録は 1 件で、答えは順に `Heard("en")`、`Outside`、`Failed`、渡した声は 32256 標本、判定の時間は 0 以上の有限の値。62 窓の区間では記録が 0 件（specified：設計 10.13.4 の手 1「1 発話に 1 回だけ」と、log の 1 行の元） | 陽性、境界 | `a_question_asked_in_the_middle_is_reported_once_with_what_it_came_to` |
| T-BR-04-013 | 答え `en`、声 32256 標本、判定 91.25 ms の記録 1 件を log の行にする | log の行 | `asr.partial.lang.asked` | event が `asr.partial.lang.asked`、level が `info`、component が `asr`。足される key は `answer`、`lid_ms`、`voice_ms` の 3 つで、値は `"en"`、91.25、2016。日英の文言表の両方にこの event の文がある（specified：observability の共通 schema と設計 10.13.4。`voice_ms` は標本数を 16 で割った整数） | 陽性 | `the_question_asked_in_the_middle_of_an_utterance_is_logged_with_what_it_came_to` |
| T-BR-04-014 | 答えが `ja`、`en`、2 つの外、失敗の 4 通りの記録 | 答えの名前 | 4 つの名前 | `answer` が順に `ja`、`en`、`outside`、`failed`（specified：同上） | 陽性 | `an_answer_outside_the_pair_and_a_failed_question_have_their_own_names` |
| T-BR-04-015 | `ja` の温まった 3 件（`L_tr` が 100、200、300 ms）と `en` の温まった 2 件（50、60 ms）の標本の報告 | 言語ごとの値 | 2 行 | `L_tr ja: n 3, p50 200 ms, p95 300 ms` と `L_tr en: n 2, p50 50 ms, p95 60 ms` の 2 行がこの順で出る（specified：BR-4 の記録は `L_tr` を言語ごとに分けた p50 と p95 を書く。nearest-rank） | 陽性 | `the_draft_translation_is_given_for_each_language_with_its_count` |
| T-BR-04-016 | `ja` の温まった 2 件だけの標本の報告 | 片方の言語が 0 件 | 0 件と言う | `en` の行が `L_tr en: 温まった標本が 0 件で、値は無い`、`ja` の行は値を持つ（specified：設計 10.13.10 の H51「分けた列の n が 0 なら、その列を通と読まない」。0 を値として書かない） | 境界、陰性 | `a_language_with_no_warm_utterance_is_said_to_have_no_value` |
| T-BR-04-017 | `en` の冷えた 1 件（`L_tr` が 999 ms）と `ja` の温まった 2 件の標本の報告 | 冷えた標本 | 数えない | `en` の行が 0 件で、`ja` の行の n が 2。冷えた 999 ms はどの行にも入らない（specified：BR-1 の「温まった標本だけ」） | 境界 | `a_cold_utterance_is_kept_out_of_the_split_by_language` |
| T-BR-04-018 | 判定の関数に、`ja` の温まった 2 件（`L_tr` が 150、60 ms）と `en` の 3 件（4、5、6 ms）の標本の報告 | 全体の p50 が線の内で、両方の言語に値が在る | p50 100 ms | `Ok` を返し、文に `p50 6 ms` を含む（specified：BR-4。5 件の並びは 4、5、6、60、150 で、nearest-rank の p50 は 3 番目） | 陽性 | `a_draft_translation_p50_inside_the_line_passes_when_both_languages_have_a_value` |
| T-BR-04-019 | `ja` の 3 件（150、160、170 ms）と `en` の 2 件（101、102 ms）の標本の報告 | 全体の p50 が線の外 | p50 100 ms | 「over the line」を含む `Err` を返す（specified：BR-4。並びは 101、102、150、160、170 で p50 は 150 ms） | 陰性 | `a_draft_translation_p50_over_the_line_fails` |
| T-BR-04-020 | `ja` と `en` が 100 ms の 1 件ずつの標本の報告。もう 1 つは 101 ms の 1 件ずつ | 線の上と線の 1 ms 上 | p50 100 ms | 前者は `Ok`、後者は「101 ms」を含む `Err`（specified：BR-4 の「100 ms 以下」。境界の両側） | 境界 | `a_draft_translation_p50_of_100_ms_is_on_the_line_and_101_ms_is_over_it` |
| T-BR-04-021 | 温まった標本が 0 件の報告 | 標本が無い | 判定できない | 「no warm value for L_tr」を含む `Err`（specified：設計 10.13.10 の H51。0 件を p50 100 ms 以下と読まない） | 境界、陰性 | `a_run_with_no_warm_draft_translation_cannot_pass_the_draft_line` |
| T-BR-04-022 | `ja` の温まった 2 件（10、20 ms）だけの標本の報告 | 片方の言語が 0 件 | 判定できない | 「no warm value for L_tr in en」を含む `Err`（specified：H51「分けた列の n が 0 なら、その列を通と読まない」） | 境界、陰性 | `a_run_in_which_one_language_has_no_warm_draft_translation_cannot_pass` |

**BR-5（確定文の訳が速報の訳の後ろに並ばない。`native/hayamimi-relay/tests/br05.rs`）**

材料：`FakeLlm` の遅れは `Call`（`stage` と `text`）で決める。確定文の T は 300 ms、E は 400 ms、速報の訳は 100 ms とする（002 の E は 600 ms、003 の速報の訳は 400 ms。遅い機械でも、送る時が待ちの間に入る）。速報の text は 3 つ（`あい`、`あいう`、`あいうえ`）で、どの行の text とも違う。依頼は、遅れの closure の中で（時刻、stage、text）を test の持つ記録へ足して、text で数える（`FakeLlm` 本体は変えない）。速報の event は `{"type":"partial","text":…}` で、source と `id` は stream の直前の `final` から中継が読む。保持は stream を読む thread が `final` を読んだ時に取るので、test は最初の `final` を settle させてから 2 つ目の `final` を送り、速報はその後ろに送る。lane は `Lanes::PerSource`。前例は T-IP-01 の `run_relay` と R8d の `r8d_partial.rs` である。

置き場は、001 から 007 と 019 の中継の行が `native/hayamimi-relay/tests/br05.rs`、008 から 018 の `Mailbox` と stream の経路の行が `native/hayamimi-relay/src/partial.rs` の `mod tests`、020 から 024 の引数の行が `native/hayamimi-relay/src/cli.rs` の `mod tests` である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-05-001 | 受入：lane `file` に `final` を送り、その T の依頼が `FakeLlm` に着いた後の 50、100、150 ms に、次の発話の速報 3 件（text が違う）を同じ lane に送る | 確定文の T が待っている間 | 速報の依頼 0 件 | 3 件の速報の text の依頼が `FakeLlm` に 0 件。`final` の translation は 1 件出る（specified：BR-5。`FakeLlm` が受けた依頼の数） | 陽性 | `br05_a_final_is_not_queued_behind_partial_translations` |
| T-BR-05-002 | T は 0 ms で返し、E が 600 ms かかる。E の依頼が着いた 100 ms 後（E の途中）に速報 1 件を同じ lane に送る | 確定文の E が待っている間 | 速報の依頼 0 件 | その速報の text の依頼が 0 件（specified：BR-5「T か E が待っている間」） | 陽性 | `a_partial_arriving_while_the_settled_translation_is_pending_is_not_requested` |
| T-BR-05-003 | 速報を送り、その訳が 400 ms かかる間（その依頼が着いた 30 ms 後）に同じ lane の `final`（text は速報の text と違う）を送る | `final` が着いた時に終わっていない速報の訳 | 捨てる | `final` が着いた後に `of: partial` の translation が 0 件（その答えは使われず、`translation` として出ない）。`final` の translation は 1 件。同じ text を閉じる `final` が答えを使い回す今の形は、今在る使い回しの test が持つ（specified：BR-5。設計 10.13.4 の手 2） | 陰性 | `an_unfinished_partial_translation_is_dropped_when_the_final_arrives` |
| T-BR-05-004 | 速報だけを 1 件送る（`final` なし） | 速報の訳が今のとおり出る | 出る | `of: partial` の translation が 1 件（specified：BR-5 の陽性対照） | 陽性 | `a_lone_partial_is_still_translated` |
| T-BR-05-005 | 001 と同じ入力で、`--partial-yield off` に当たる設定 | 旗を切る | 今の形 | 3 件の速報の text のうち 1 件以上の依頼が、`final` の T の待ちの間に `FakeLlm` に届く（specified：BR-5 の陰性対照） | 陰性 | `with_partial_yield_off_a_partial_is_requested_while_the_final_waits` |
| T-BR-05-006 | `final` の T と E がすべて済み、確定の translation が出た後（`settled: true` を受けた後）に、次の速報を送る | 待ちが終わった境界 | 譲るのは待つ間だけ | その速報の依頼が 1 件（lane は空いている。以後の速報が止まり続けない）（derived：設計 10.13.4 の手 2「待っている間は」の反対側） | 境界 | `a_partial_after_the_final_is_settled_is_requested_again` |
| T-BR-05-007 | lane `file` の `final` の T が待つ間に、lane `mic` に速報を 1 件送る | 別の lane | 同じ lane だけ | `mic` の速報の依頼が 1 件（specified：設計 10.13.4 の手 2「同じ lane」） | 陰性、境界 | `a_partial_in_another_lane_is_not_held_by_the_final_of_this_lane` |
| T-BR-05-008 | 保持を取る `Mailbox`。`mic` の保持を取り、`mic` の text を `post`。続けて `loopback` の text を `post` して `take` | 保持の間 | 捨てる | 1 つ目の `post` が `Yielded`。`take` が返すのは `loopback` の text（`mic` の text は待っていない）（derived：設計 10.13.4 の手 2「溜めずに捨てる」） | 陽性 | `a_text_is_let_go_while_a_final_of_its_source_is_held` |
| T-BR-05-009 | 008 の `mic` の代わりに `file` の保持を取り、`mic` の text を `post` | 別の source | 同じ source だけ | `Left`（derived：同上） | 陰性 | `the_text_of_another_source_is_not_let_go` |
| T-BR-05-010 | `mic` の保持を取り、text を `post`（`Yielded`）。保持を返し、同じ text をもう一度 `post` | 保持が解けた境界 | 解ければ受ける | 2 つ目の `post` が `Left` で、`take` がその text を返す。捨てた text は覚えられていない（derived：同上） | 境界 | `a_text_is_left_again_once_the_hold_is_dropped` |
| T-BR-05-011 | `mic` の保持を 2 つ取り、1 つ返して `post`。もう 1 つ返してもう一度 `post` | 保持の数 | 0 になるまで捨てる | 1 つ返した後は `Yielded`、2 つとも返した後は `Left`（derived：1 つの source に `final` が重なる時） | 境界 | `a_source_is_held_until_every_final_in_its_lane_is_settled` |
| T-BR-05-012 | 保持を取らない `Mailbox`（`Mailbox::new()`）で `settling` と `post` | 旗を切る | 保持なし | `settling` が `None`、`post` が `Left`（specified：BR-5 の陰性対照の機構） | 陰性 | `a_mailbox_that_does_not_yield_holds_nothing` |
| T-BR-05-013 | 取っていない保持を返し、続けて `post` | 保持が 0 件 | 0 のまま | `post` が `Left`（derived：数は 0 から下がらない） | 境界 | `giving_back_a_hold_that_was_never_taken_leaves_the_source_free` |
| T-BR-05-014 | 保持を取る stream の経路。`final`（`mic-1`）を `note_line` し、`partial` で text を送り、保持を返してからもう一度 `partial` で送る | stream が読む順 | 保持の間だけ捨てる | 1 回目は log が 1 件（`translate.partial.skipped`、`reason` が `yielded`）、2 回目は log 0 件で `take` の `id` が `mic-2`（specified：設計 10.13.4 の手 2 の log の 1 行） | 陽性 | `a_final_holds_its_source_and_a_let_go_text_is_logged_as_yielded` |
| T-BR-05-015 | `refine` を `note_line` | `final` でない行 | 保持なし | `None`（specified：設計 10.13.4 の手 2「保持を持つのは `final` だけ」） | 陰性 | `a_refine_takes_no_hold` |
| T-BR-05-016 | 保持を取らない `Mailbox` に向く経路で `final` を `note_line` | 旗を切る | 保持なし | `None`（specified：同上） | 陰性 | `a_route_into_a_mailbox_that_does_not_yield_takes_no_hold` |
| T-BR-05-017 | `Mailbox` の無い経路で `final` を `note_line` | 速報を訳さない run | 保持なし | `None`（derived：`--no-partial-translation` の run は保持を取らない） | 陰性 | `a_route_into_no_mailbox_takes_no_hold` |
| T-BR-05-018 | 速報を 1 件 `take` して走らせた後、同じ source の次の `final` を `note_line` | `final` が走っている速報を止める今の形 | 止める | `Wanted::still` が偽で、保持は取れる（derived：保持を足しても `final` が着いた時の捨て方は変わらない） | 陽性 | `the_final_still_stops_the_text_in_progress_while_it_holds_its_source` |
| T-BR-05-019 | 001 と同じ入力で、log を見る | 捨てた text の log | 3 件 | `translate.partial.skipped` が 3 件で、`reason` がすべて `yielded`（specified：設計 10.13.4 の手 2 の log の 1 行） | 陽性 | `the_texts_in_progress_that_are_let_go_are_said_so_in_the_log` |
| T-BR-05-020 | `live` の引数：何も付けない、`--partial-yield off`、`--partial-yield on` | 既定と旗 | 既定は保持あり | 順に `Yielding`、`On`、`Yielding`。`--no-partial-translation` は `Off`（specified：設計 10.13.4 の手 2 の戻し方） | 陽性 | `the_translation_of_the_text_in_progress_yields_unless_it_is_turned_off` |
| T-BR-05-021 | `live` の引数が `--partial-yield off` | 旗を切る | 譲らない | `On`（速報の訳は切らず、譲らない）（specified：BR-5 の陰性対照） | 陰性 | `partial_yield_off_translates_the_text_in_progress_without_yielding` |
| T-BR-05-022 | `--no-partial-translation` と `--partial-yield on`、`off` を、順を替えた 4 通りで並べる | 旗の組 | 訳を切る旗が勝つ | 4 通りとも `Off`（derived：切った訳は譲る相手を持たない） | 境界 | `turning_the_translation_off_wins_over_the_yield_in_either_order` |
| T-BR-05-023 | `--partial-yield` の値が `maybe`、空、`ON`、`1`、値なし | 不正な値 | 断る | 値があれば `BadPartialYield`、値なしは `MissingValue`（derived：`--partial-lid` の `on`、`off` だけの規則と同じ） | 陰性 | `the_yield_takes_on_or_off_and_nothing_else` |
| T-BR-05-024 | `BadPartialYield` の鍵を日英の文言表で引く | 断る文 | 両方に在る | 日英とも文言が在る（鍵 `bad_partial_yield`）。今在る test `the_run_refusals_have_text_in_both_locales` の鍵の一覧に足す（specified：observability の共通 schema） | 陽性 | `the_run_refusals_have_text_in_both_locales` |

**BR-6（区間を閉じた時の遅れの log。`native/hayamimi/src/log.rs`、`native/hayamimi-asr/src/vad.rs`）**

材料：log の 1 行は JSON で、標準の項目（`ts`、`level`、`event` ほか。`log.rs` の今の行と同じ）の外に、この event が足す項目を `id`、`lag_ms`、`rule` の 3 つだけにする。`every_event_the_code_emits_has_a_message_in_both_tables`（今在る test）が、新しい event の文言を `strings.en.toml` と `strings.ja.toml` の両方に求める。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-06-001 | 受入：合成の区間 1 つ（`id` が `file-3`、受けた標本 48000、VAD に渡した標本 32000、閉じた detector が主、`silence`）から行を作る | 1 区間 | 1 件 | `event` が `asr.vad.closed` の行がちょうど 1 件。`level` が `info`。足された項目が `id`、`lag_ms`、`rule` の 3 つだけで、値が `"file-3"`、1000、`"silence"`（specified：BR-6） | 陽性 | `br06_the_close_of_a_segment_logs_how_far_the_detector_lagged` |
| T-BR-06-002 | 受けた標本から渡した標本を引いた差が (16, 160, 48000) の 3 区間 | ms への直し | 16 標本 = 1 ms | `lag_ms` が 1、10、3000。続けて差が 15、17、31 標本の区間は 0、1、1（specified：設計 10.13.13 の 1 の floor。16000 Hz で 1 標本は 62.5 µs） | 陽性 | `the_lag_is_the_sample_difference_in_milliseconds_at_16000_hz` |
| T-BR-06-003 | 差が 0 標本の区間 | 遅れ 0 | 0 は数 | `lag_ms` の項目が在り、値が数の 0（`null` でも欠けでもない）（specified：BR-6「遅れ」。0 を「項目なし」と読まない） | 境界 | `a_lag_of_zero_is_written_as_the_number_zero` |
| T-BR-06-004 | 閉じた detector が (主の無音、早い detector、`max_speech` の強制) の 3 区間 | `rule` の 3 つの値 | 3 つ | `rule` が `silence`、`early`、`max_speech`（specified：BR-6） | 陽性 | `the_rule_names_which_detector_or_limit_closed_the_segment` |
| T-BR-06-005 | `final` にならなかった区間（decode が空）と、`final` になった区間（`file-7`） | 区間が `final` になる、ならない | 区間ごとに 1 件 | どちらも 1 件ずつ出る。前者の `id` は `null`（項目は在る）、後者は `"file-7"`（specified：BR-6 の陰性対照と 10.9.4） | 陰性、境界 | `a_segment_that_made_no_final_is_logged_with_a_null_id` |
| T-BR-06-006 | 区間 3 つを続けて閉じる | 件数 | 区間の数と同じ | `asr.vad.closed` が 3 件で、`lag_ms` が閉じた順に入力の差と等しい（property：1 区間 1 件。重複なし） | property | `one_line_is_written_per_closed_segment` |
| T-BR-06-007 | 実物：`vad.rs` の `mod tests`。silero の model と、無音で終わる声の clip を `Vad` に流す（0.30 秒の無音で閉じる） | 主の detector が閉じる | `silence` | 閉じた区間の `rule` が `silence`（specified：BR-6。model の置き場は今の `models_dir()` と同じ） | 陽性 | `a_segment_closed_by_the_pause_is_ruled_silence` |
| T-BR-06-008 | 実物：`max_speech` を 1.0 秒に設定し、3 秒続く声を流す | 強制で閉じる | `max_speech` | 閉じた区間の `rule` が `max_speech` | 境界 | `a_segment_cut_at_the_longest_speech_is_ruled_max_speech` |
| T-BR-06-009 | 実物：早い detector を開いて（`open_with_early`）、1000 ms 以上の声の後に 120 ms 以上の無音を置く | 早い detector が閉じる | `early` | 閉じた区間の `rule` が `early` | 境界 | `a_segment_closed_by_the_early_detector_is_ruled_early` |

**BR-7（E の n-gram の下書きの効き。記録。`native/hayamimi-relay/tests/br07.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-07-001 | 受入、実走：`--spec-type ngram-simple` を付けた 18084 と付けない 18084 を別々に起こし、調整用の 40 行（3.5）を温度 0 で通す | 旗の有無 | 記録だけ | 記録に、E の段の p50 と p95 を 2 本で並べ、確定の文が 40 行で一致した数を書く。落とさない（specified：BR-7。数える関数が正しいことだけが合否） | 陽性 | `br07_ngram_drafting_keeps_the_settled_text_and_records_the_e_stage` |
| T-BR-07-002 | 実走：旗の無い 18084 を 2 回起こして同じ 40 行を通す | 旗の無い 2 回 | 一致 40 | 一致した数が 40（specified：BR-7 の陽性対照。決定的でなければ 40 にならず、記録に「旗の無い 2 回で 40 にならない」と書く） | 陽性 | `two_runs_without_the_flag_agree_on_all_forty_lines` |
| T-BR-07-003 | 道具：40 行のうち 1 行の文が違う合成の組 | 1 行違い | 一致の数 | 「一致 39」と書く（specified：BR-7 の陰性対照） | 陰性 | `a_pair_that_differs_on_one_line_is_written_as_39_agreeing` |
| T-BR-07-004 | 道具：40 行が全部同じ組と、0 行の組 | 全部一致、空 | 空は誤り | 前者は「一致 40」、後者は `Err`（implicit：0 件を一致 0 と読まない） | 境界、陰性 | `an_empty_pair_of_runs_is_an_error_and_not_zero_agreeing` |
| T-BR-07-005 | 道具：E の段の ms が 100、200、300、400 の 4 件 | nearest-rank n = 4 | p50 は 2 番目、p95 は 4 番目 | p50 が 200、p95 が 400（derived：4.1） | 陽性 | `the_e_stage_percentiles_are_nearest_rank` |
| T-BR-07-006 | 道具：旗つきの server の起動の形を作る関数に、port 18084 と 18081 を渡す | 旗を当てる port | 18084 だけ | 18084 は `--spec-type ngram-simple` と `--port 18084` を含む形を返す。18081 は `Err`（共有の server に旗を当てない）。旗なしの形はどちらの port でも旗を含まない（specified：設計 10.13.1「共有の llama-server（127.0.0.1:18081）の旗の変更」を作らない物とする。IP-S9「旗は run の 18084 だけに当て」） | 陰性 | `the_flag_is_never_built_for_the_shared_server_port` |

**BR-8（短い声と叫びが区間になるか。記録。`native/hayamimi-asr/tests/br08.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-08-001 | 受入、実物：`MIN_SPEECH_SECONDS` 0.25、0.15、0.10 と閾値 0.5、0.4 の 6 つの組で、`eval_real` の全 clip、`lat-dialog`、`lat-short` を VAD に通す | 6 つの組 | 記録だけ | 記録に 6 つの組が、(0.25, 0.5)、(0.25, 0.4)、(0.15, 0.5)、(0.15, 0.4)、(0.10, 0.5)、(0.10, 0.4) の順に在り、組ごとに区間の数、区間の無い発話の一覧、隣の発話を飲んだ区間の数を書く（specified：BR-8） | 陽性 | `br08_short_and_shouted_voices_are_counted_with_the_neighbours_they_swallow` |
| T-BR-08-002 | 実物：001 の (0.25, 0.5) の組の記録 | 今の組 | 今の値 | `lat-dialog` の行 3 と `lat-short` の行 1、5、7、14 が区間の無い発話の一覧に入る（specified：BR-8 の陽性対照） | 陽性 | `the_current_setting_leaves_the_known_shouted_and_short_voices_without_a_segment` |
| T-BR-08-003 | 道具：manifest の 1 発話 [0, 48000) を、区間 [0, 20000) と [28000, 48000) の 2 つに分けた合成 | 1 発話が 2 つの区間 | 飲み込み 0 | 区間の数が 2、隣の発話を飲んだ区間が 0、区間の無い発話が 0 件（specified：BR-8 の陰性対照） | 陰性 | `one_utterance_split_into_two_segments_swallows_nothing` |
| T-BR-08-004 | 道具：発話 [0, 16000) と [20000, 36000) に対し、区間 [8000, 30000) 1 つ | 1 つの区間が 2 発話に掛かる | 飲み込み 1 | 隣の発話を飲んだ区間が 1、区間の無い発話が 0 件（specified：設計 10.13.13 の 19「2 つ以上の発話と重なる区間」） | 陽性 | `a_segment_over_two_utterances_counts_as_swallowing_a_neighbour` |
| T-BR-08-005 | 道具：発話 [100, 200) に対し、区間 [0, 100) の 1 つ | 区間の終わりが発話の始まりに接する | 掛からない | 区間の無い発話が 1 件（半開区間。1 標本も重ならない）（specified：設計 10.13.13 の 19「端が接するだけは重ならない」） | 境界 | `a_segment_that_only_touches_an_utterance_does_not_cover_it` |
| T-BR-08-006 | 道具：発話 [100, 200) に対し、区間 [0, 101) の 1 つ | 1 標本だけ重なる | 掛かる | 区間の無い発話が 0 件（上側。設計 10.13.13 の 19「共通の標本が 1 つでも在れば重なる」） | 境界 | `a_segment_that_overlaps_an_utterance_by_one_sample_covers_it` |
| T-BR-08-007 | 道具と実物：`rg -n 'MIN_SPEECH_SECONDS: f32 = 0.25\|const THRESHOLD: f32 = 0.5' native/hayamimi-asr/src/vad.rs`、陽性対照として `rg -c 'MIN_SPEECH_SECONDS' native/hayamimi-asr/src/vad.rs` | VAD の線は動かさない | 2 件 | 前者が 2 件、後者が 1 以上（specified：IP-S11「VAD の線と判定の手は変えない」。BR-8 は記録を材料に評議の席が決めるまで線を動かさない） | 陽性、陰性 | `the_vad_line_is_not_moved_by_the_record_tool` |

**BR-9（final の無い繰り返しと `sv_tag` の内訳。記録。`native/hayamimi/tests/br09.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-09-001 | 受入、実物：`br09_ip7_pair.jsonl`、`br09_ip7_pair.events.jsonl`、`br09_ip7_pair.log` に記録の関数を当てる | final の無い繰り返し 16 件 | 3 つの和が 16 | 「窓の外に割り当たった」「空に読んだ」「区間が無い」の 3 つに、設計 10.13.13 の 21 の順（(a)、(b)、(c)）で分けて数え、和が 16。「区間が無い」は 12（記録の `utterances with no segment: [1, 5, 7, 14]` の 4 発話 × 3 repeat）、「空に読んだ」は 0（記録の `asr.segment.unread` の 2 行がともに 0）、「窓の外に割り当たった」は残りの 4（16 − 12 − 0）。16 件のそれぞれが repeat と utt の番号で名指される（specified：BR-9 と設計 10.13.13 の 21。derived：12 と 0 は既存の記録の行から、4 は和から引いた残り） | 陽性 | `br09_each_repeat_without_a_final_is_given_its_reason` |
| T-BR-09-002 | 実物：`br09_ip7.md` の `repeats with no final:` の行の値 | 記録の既存の数 | 一致 | 001 の 3 つの和が、この行の値（16）と一致する（specified：BR-9 の陽性対照と設計 10.13.13 の 21「3 つの和が…等しい」） | 陽性 | `the_three_reasons_add_up_to_the_repeats_with_no_final_of_the_record` |
| T-BR-09-003 | 道具：和が合わない合成の記録（final の無い繰り返しの申告が 16 で、分けた和が 15） | 和が合わない | 一致 | `Err`（specified：BR-9 の陰性対照と設計 10.13.13 の 21「和が合わない時だけ `Err`」） | 陰性 | `reasons_that_do_not_add_up_to_the_repeats_without_a_final_are_an_error` |
| T-BR-09-004 | 道具：合成の記録。(a) 発話の範囲に重なる `final` が在り、その `final` が `build_samples` で別の発話に割り当たった repeat、(b) 範囲に重なる `asr.segment.unread` の `empty_decode` の行が在る repeat、(c) 範囲に重なる `final` も `unread` も無い repeat | 3 つの理由の代表 | 1 repeat に 1 つ | (a) が「窓の外に割り当たった」、(b) が「空に読んだ」、(c) が「区間が無い」。各 1 件（specified：設計 10.13.13 の 21 の (a)、(b)、(c)） | 陽性 | `each_reason_is_told_apart_on_a_hand_made_record` |
| T-BR-09-007 | 道具：範囲に重なる `final`（別の発話に割り当たった）と、同じ範囲に重なる `asr.segment.unread` の両方が在る repeat | 2 つの理由が同時に当たる | 順に最初の物 | 理由は「窓の外に割り当たった」の 1 つだけで、「空に読んだ」には数えない（specified：設計 10.13.13 の 21「次の順に最初に当たった物」） | 境界 | `the_first_matching_reason_in_order_is_the_only_one_counted` |
| T-BR-09-008 | 道具：T-BR-09-004 の (a)、(b)、(c) の repeat が 1 件ずつの合成の記録（`repeats with no final` は 3） | 和が申告より 1 多い | 申告 16 に対し和 17 | 一致 | `Err`（T-BR-09-003 の 15 と対になる、和が上に外れる側）。T-BR-09-004 の合成の記録（申告 3、和 3）は `Err` でなく、数える欄は 3 つだけで 4 つ目の欄は無い（specified：設計 10.13.13 の 21「3 つの和が…等しく、和が合わない時だけ `Err`」） | 境界 | `reasons_that_add_up_above_the_repeats_without_a_final_are_an_error` |
| T-BR-09-005 | 実物：`br09_ip7_pair.log` の `asr.lang.fallback` の `step` が `sv_tag` の行 | `sv_tag` の 14 件 | 14 | 14 行が出て、発話ごとに `L_asr` と、同じ発話の `lid` の答えを持つ。件数は記録の `asr.lang.fallback sv_tag: 14` と一致（specified：BR-9。derived：記録の行） | 陽性 | `each_sv_tag_fallback_is_written_with_its_wait_and_its_lid_answer` |
| T-BR-09-006 | 道具：`sv_tag` の行を持たない合成の記録 | `sv_tag` が 0 件 | 0 件を成果と読まない | 内訳は 0 行で、「sv_tag なし」と書く（001 と 005 が非 0 の陽性対照） | 陰性 | `a_record_without_sv_tag_lines_says_so` |

**BR-10（取り違えた発話の名指し。記録。`native/hayamimi/tests/br10.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-10-001 | 受入、実物：`br10_ip2_pair.*`、`br10_ip2_fast.*`、`br10_ip7_pair.*` に記録の関数を当てる | 取り違えた発話 0 件、1 件、1 件 | 記録の数と同じ行数 | IP-2 の pair は 0 行、IP-2 の fast は 1 行（第三の言語）、IP-7 の pair は 1 行（`ja` を `en`）。各行に manifest の `lang`、whisper-tiny の答え、SenseVoice の印（在れば）、10.4 の `step`、声の長さ（ms）がある（specified：BR-10） | 陽性 | `br10_each_mixed_up_utterance_is_named_with_both_identifiers_answers` |
| T-BR-10-002 | 実物：`br10_ip2.md` と `br10_ip7.md` の `mixed-up utterances:` の行 | 記録の既存の数 | 一致 | 001 の行数が、それぞれの行の先頭の数（0、1、1）と一致（specified：BR-10 の陽性対照） | 陽性 | `the_named_rows_are_as_many_as_the_mixed_up_utterances_of_the_records` |
| T-BR-10-003 | 道具：取り違えの無い合成の記録 | 0 件 | 0 行 | 0 行で、「取り違えなし」と書く（specified：BR-10 の陰性対照） | 陰性 | `a_record_without_mix_ups_names_no_utterance` |
| T-BR-10-004 | 道具：`asr.lang.fallback` の行を持たない取り違えた発話 | 印が無い | 空文字で埋めない | SenseVoice の印と `step` の欄は「なし」と書き、空文字や 0 でない（derived：欠けを既定で埋めない規約。001 の行が印を持つ陽性対照） | 境界 | `a_missing_sensevoice_tag_is_written_as_none` |
| T-BR-10-005 | 道具：声の長さが 15999、16000、32000 標本の 3 発話 | 声の長さの単位 | 16 標本 = 1 ms | 999 ms、1000 ms、2000 ms（specified：設計 10.13.13 の 1 の floor。15999 標本は 999937.5 µs） | 陽性 | `the_voice_length_is_written_in_milliseconds` |

**BR-11（IP-4 の結べない clip の理由。`native/hayamimi-eval/tests/br11.rs`）**

材料：clip の結び方は `ip04.rs` の `medians_by_clip` と `deltas` と同じ（sha256 で結び、閉じる `final` の無い repeat は外し、残りの中央値を取る）。理由は clip の 3 repeat を見て付ける。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-11-001 | 受入：合成の pair の run（5 clip × 3 repeat）と、全 clip が閉じた `--lang` の run。clip 1 は全 repeat が閉じる。clip 2 は全 repeat の `finals` が 0。clip 3 は全 repeat で `finals` が 1 で最後の `final` が `t_end` より前。clip 4 は全 repeat で `finals` が 2 で最後が `t_end` より前。clip 5 は 3 repeat のうち 1 つだけ閉じる | 理由の 2 つと結べる clip | 名指す理由は 2 つ | 結べた clip は 1 と 5、名指しは clip 2 が `no_final`、clip 3 と 4 が `closed_before_end`。結べた 2 と名指した 3 の和が 5。理由の語は逐語でこの 2 つ（specified：BR-11） | 陽性 | `br11_every_clip_of_the_pair_run_is_joined_or_named_with_its_reason` |
| T-BR-11-002 | 実物：`ip04_pair.jsonl` と manifest（`lat-dialog`）に記録の関数を当てる | 実際の run | 30 clip | 行 3 が `no_final`、行 9、19、21 が `closed_before_end`。結べた clip が 26、名指した clip が 4（specified：BR-11。derived：`jq` で 2026-10-23 に数えた `finals` が 0、1、2、1） | 陽性 | `the_pair_run_names_row_3_as_no_final_and_rows_9_19_21_as_closed_before_end` |
| T-BR-11-003 | 002 の結果 | 結べた + 名指した | 母数 30 | 26 + 4 = 30（specified：BR-11 の陽性対照。ここが 30 でなければ clip が漏れている） | 陽性 | `joined_and_named_clips_add_up_to_the_clips_of_the_manifest` |
| T-BR-11-004 | 道具：manifest は 5 行だが記録に 4 発話分の行しか無い合成（1 clip の行が全 repeat で欠ける） | 理由を付ける材料が無い clip | 理由が 1 つも残らない記録は誤り | `Err`（理由の無い clip を名指して言う）（specified：BR-11 の陰性対照） | 陰性 | `a_clip_with_no_reason_left_makes_the_record_an_error` |
| T-BR-11-005 | 道具：最後の `final` の `t_us` が `t_end_us` と等しい発話と、1 µs 前の発話 | `t_end` ちょうど | `>=` で閉じる | 前者は閉じる `final` を持つ（結べる）。後者は `closed_before_end`（derived：`build_samples` の `events[i].t_us >= window.t_end_us`。設計 10.13.2「最後の sample を送る前に着いた」の両側） | 境界 | `a_final_arriving_exactly_at_the_end_closes_the_utterance` |
| T-BR-11-006 | 道具：1 つの clip で閉じない repeat が 0 個、1 個、2 個、3 個の 4 通り | repeat の欠け | 3 個すべて欠けた時だけ名指す | 0、1、2 個は結べる（残りの repeat の中央値）、3 個は名指される（derived：`medians_by_clip` が欠けた repeat を外す。「IP-4 の線と結び方は変えない」） | 境界 | `a_clip_is_named_only_when_every_repeat_lacks_a_closing_final` |
| T-BR-11-007 | 道具：pair の run では閉じ、`--lang` の run の全 repeat で `finals` が 0 の clip | 欠けた側が `--lang` の run | 理由を付ける | その clip が名指され、理由は `missing_in_lang_run`。どちらの `--lang` の run（`--lang ja` か `--lang en`）で欠けたかと、その run の側の理由 `no_final` が並んで書かれ、pair の run の側の理由の欄には書かれない。同じ入力で `--lang` の run の `finals` が 1 以上なら、run の側の理由は `closed_before_end`（specified：設計 10.13.13 の 3） | 境界 | `a_clip_missing_in_the_forced_language_run_is_named_with_its_reason` |
| T-BR-11-008 | 道具：pair の run の 1 つの clip で、repeat 1 は `finals` 0、repeat 2 と 3 は `finals` 1 で最後の `final` が `t_end` より前 | repeat ごとに理由が違う | `mixed` | clip の理由が `mixed: 1 no_final, 2 closed_before_end, 3 closed_before_end`（repeat の順、番号と理由の組をすべて並べる）（specified：設計 10.13.13 の 3） | 陽性 | `a_clip_whose_repeats_lack_for_different_reasons_is_named_mixed_with_each_reason` |
| T-BR-11-009 | 道具：pair の run の 1 つの clip で、3 repeat とも最後の `final` が `t_end` より前で、`finals` は 1、2、1 | 理由が同じで `finals` の数だけが違う | 理由を 1 つだけ書く | clip の理由が `closed_before_end` の 1 つだけで、`mixed` でない。同じく 3 repeat とも `finals` 0 の clip は `no_final` の 1 つだけ（specified：設計 10.13.13 の 3「3 つの repeat が同じ理由なら、その理由を 1 つだけ書く」） | 陰性 | `a_clip_with_the_same_reason_in_every_repeat_is_not_mixed` |
| T-BR-11-010 | 道具：pair の run では閉じ、`--lang ja` の run の 3 repeat が `no_final`、`closed_before_end`、`no_final` の clip | `--lang` の run の側で理由が混ざる | `missing_in_lang_run` の側も同じ形 | 理由が `missing_in_lang_run`、run が `--lang ja`、その run の側の理由が `mixed: 1 no_final, 2 closed_before_end, 3 no_final`（specified：設計 10.13.13 の 3「`missing_in_lang_run` の側の理由も、同じ形で repeat ごとに並べる」） | 境界 | `the_lang_run_side_of_a_missing_clip_lists_its_repeats_the_same_way` |
| T-BR-11-011 | 道具：7 clip の記録。clip 1 と 2 は両方の run で閉じる。clip 3 は pair の run の全 repeat で `no_final`。clip 4 は全 repeat で `closed_before_end`。clip 5 は pair の run の repeat 1 が `no_final`、2 と 3 が `closed_before_end`。clip 6 は `--lang ja` の run の全 repeat で `no_final`。clip 7 は pair の run の 3 repeat のうち 2 つが `no_final` で 1 つだけ閉じる | 理由の数え上げ | 和が結べない clip の数 | 結べた clip が 3（1、2、7）、`no_final` が 1、`closed_before_end` が 1、`mixed` が 1、`missing_in_lang_run` が 1。この 4 つの理由の和が 4 で、結べない clip の数（7 − 3 = 4）に等しい。どの clip も 1 度だけ数える（specified：設計 10.13.13 の 3「4 つの和が結べない clip の数と等しい」） | property | `the_reasons_add_up_to_the_clips_that_could_not_be_joined` |
| T-BR-11-012 | 道具：1 つの clip で、pair の run の全 repeat が `no_final`、`--lang ja` の run の全 repeat が `closed_before_end` | 両方の run で欠ける | `missing_in_both` | clip の理由が `missing_in_both`。pair の run の側の理由 `no_final` と、`--lang ja` の run の側の理由 `closed_before_end` が並んで書かれる。`no_final` にも `missing_in_lang_run` にも数えない（specified：設計 10.13.13 の 14「片方の理由に寄せない」） | 陽性 | `a_clip_missing_in_both_runs_is_named_missing_in_both_with_each_side` |
| T-BR-11-013 | 道具：8 clip の記録。clip 1 と 2 は両方の run で閉じる。clip 3 は pair の run の全 repeat で `no_final`。clip 4 は全 repeat で `closed_before_end`。clip 5 は pair の run で `mixed`。clip 6 は `--lang ja` の run だけ全 repeat で `no_final`。clip 7 は pair の run の 3 repeat のうち 2 つが `no_final` で 1 つだけ閉じる。clip 8 は両方の run の全 repeat で `no_final` | 5 つの理由の数え上げ | 和が結べない clip の数 | 結べた clip が 3（1、2、7）、`no_final`、`closed_before_end`、`mixed`、`missing_in_lang_run`、`missing_in_both` が各 1。5 つの和が 5 で、結べない clip の数（8 − 3 = 5）に等しい。どの clip も 1 度だけ数える（specified：設計 10.13.13 の 3 と 14「5 つの和が結べない clip の数と等しい」） | property | `the_five_reasons_add_up_to_the_clips_that_could_not_be_joined` |
| T-BR-11-014 | 道具：1 つの clip で、pair の run の repeat が `no_final`、`closed_before_end`、`no_final`、`--lang en` の run の全 repeat が `no_final` | 両方の run、pair の側が `mixed` | 3 の形を両側に使う | 理由が `missing_in_both`。pair の run の側が `mixed: 1 no_final, 2 closed_before_end, 3 no_final`、`--lang en` の run の側が `no_final`（specified：設計 10.13.13 の 14「repeat ごとの理由。3 の形」） | 境界 | `both_sides_of_a_missing_in_both_clip_use_the_per_repeat_form` |

**BR-12（IP-4 の線の外の clip の道。記録。`native/hayamimi-eval/tests/br12.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-12-001 | 受入、実物：`ip04_pair.*`、`ip04_lang_ja.*`、`ip04_lang_en.*` と pair の log に記録の関数を当てる | 結べた 26 clip | 差の大きい順 | 26 行が差（pair の中央値 − `--lang` の中央値）の大きい順に並び、全部の行に `asr.lang.fallback` の `step`、`correct_by_script` の読み直しの有無、`lid_ms` がある。差が 300 ms を超える行だけに `over` の印が付き、その行は 2 件以上（p95 が 381 ms で、上から 2 番目までが 381 ms 以上のため）（specified：BR-12 と設計 10.13.13 の 22。derived：`over` の件数の下限は p95 から） | 陽性 | `br12_the_clips_over_the_ip04_line_are_named_with_their_language_path` |
| T-BR-12-002 | 実物：001 の差の p95 | 記録の既存の数 | 381 ms | 差の p95 が 381 ms（`ip04.md` の `delta p95: 381 ms`）と一致（specified：BR-12 の陽性対照） | 陽性 | `the_delta_p95_of_the_named_clips_is_the_381_ms_of_the_record` |
| T-BR-12-003 | 道具：`asr.lang.fallback` の行も読み直しの行も無い clip | 道の印が無い | 空で埋めない | その clip の欄は「印なし」と書かれる（specified：BR-12 の陰性対照） | 陰性 | `a_clip_without_any_path_mark_is_written_as_no_mark` |
| T-BR-12-004 | 道具：差が 50、400、120、300 ms の 4 clip | 並び | 降順 | 行の並びが 400、300、120、50（property：任意の差の列で降順。差が等しい clip は置かない） | property | `the_clips_are_listed_from_the_largest_delta_down` |
| T-BR-12-005 | 道具：`step` が `sv_tag`、読み直しが有り、`lid_ms` が 91 の clip と、`step` は有るが読み直しが無い clip | 印の組合せ | 在る物を全部書く | 前者は 3 つとも書かれ、後者は読み直しが「なし」（derived） | 境界 | `each_mark_of_a_clip_is_written_and_a_missing_reread_is_written_as_none` |
| T-BR-12-006 | 道具：差が 299、300、301 ms の 3 clip（道の印は全部に付く） | `over` の線 | 300 ms を超える差 | 301 ms の clip だけに `over` が付き、300 と 299 には付かない。3 clip とも道の印が付く（specified：設計 10.13.13 の 22「300 ms を超える差の clip に `over`、道の印は全部の clip に」） | 境界 | `only_a_delta_over_300_ms_is_marked_over_and_every_clip_keeps_its_path_marks` |

**BR-13（同じ session への 2 回目の run の `final`。`native/hayamimi-eval/tests/br13.rs`）**

材料：記録 C は 2 回目の run の合成で、`session_start` が無く、`ready` が `t_us` 50000 に在り、`ready` は `stream_s` 30.904（接続した時の流れの時計の位置）を持つ。道具は `ready` を受けた時刻から `stream_s` を引いた値を、その接続の `t1_s` を道具の時計へ寄せる量にする（設計 10.13.13 の 10）。4 発話の窓は `(t_start_us, t_end_us)` が (1000000, 3000000)、(6000000, 8000000)、(11000000, 13000000)、(16000000, 18000000)、言語は `ja`、`en`、`ja`、`en`。`final` は `id` が `file-1` から `file-4`、`t_us` が `t_end_us + 400000`、`latency_ms` 300、`t1_s` が `t_end_us / 1000000 + 30.854`（33.854、38.854、43.854、48.854）。記録 D は記録 C の `t1_s` から 30.904 を引き、`session_start` を `t_us` 50000 に置き、`ready` から `stream_s` を取った 1 回目の形（今の記録）である。割り当ては `build_samples` の規則で、記録の関数は `native/hayamimi/tests/support/dialog.rs` の `given_of` と同じ入口（2 つの file の text を受ける）で呼ぶ。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-13-001 | 受入：記録 C を、起点を置き直す記録の関数に渡す | `session_start` の無い 2 回目の run | 1 窓に 1 件 | 4 件の `final` が窓 1 から 4 に 1 件ずつ割り当たる（`finals` が 1、1、1、1）。要約の `with_final` が 4（specified：BR-13 と設計 10.13.13 の 10。`t1_s` 33.854 は `0.05 − 30.904 + 33.854 = 3.0` 秒に寄り、窓 1 の終わりに等しい） | 陽性 | `br13_a_second_stream_into_one_session_places_its_finals_on_its_own_windows` |
| T-BR-13-002 | 記録 D を同じ関数に渡す | `session_start` が在る 1 回目の形 | 今のとおり | `finals` が 1、1、1、1。`with_final` が 4（specified：BR-13 の陽性対照） | 陽性 | `a_first_stream_with_a_session_start_is_still_placed_as_before` |
| T-BR-13-003 | 記録 C を、起点を置き直さない `build_samples` に、`final` の `t1_s` をそのまま置いて渡す | 今の不具合 | 4 件が最後の窓 | `finals` が 0、0、0、4、`with_final` が 1（derived：2026-10-23 の実際の 2 回目の run の記録で、行 4 の `finals` が 4、行 1 から 3 が 0。設計 10.13.5 の Q-g。この対照が通らなくなる日は、`build_samples` 自体を直した日で、その時は設計の席に戻す） | 陰性 | `without_a_new_origin_the_four_finals_fall_on_the_last_window` |
| T-BR-13-004 | 記録 C の `t1_s` にさらに 30.854 を足し、`stream_s` を 61.758 にして（3 回目の run）、同じ関数に渡す | 前の run が 2 回 | 1 窓に 1 件 | `finals` が 1、1、1、1（property：前の run の数に依らず、`ready` の `stream_s` による寄せが働く） | property | `a_third_stream_into_one_session_places_its_finals_on_its_own_windows` |
| T-BR-13-005 | 記録 C の参照文を 4 件の `final` の text に等しく置き、要約を出す | 文字の照合 | 完全一致 | `cer_dist` の和が 0。今の不具合の要約（2026-10-23 の実測 `cer_micros` 1641304）の値にならない（pseudo：参照文と `final` の text が同じなので 0 になる） | 陽性 | `the_summary_of_a_second_stream_has_no_character_errors_when_the_texts_agree` |
| T-BR-13-006 | 記録 C の `ready` から `stream_s` を取り、`session_start` を `t_us` 50000 に置いた記録。もう 1 つは `stream_s` も `session_start` も無い記録 | `stream_s` が無い今の記録 | 今のとおり `session_start` で寄せる | 前者は記録 D と同じ割り当て（`finals` が 1、1、1、1）。後者は寄せる量が 0 で、`finals` が 0、0、0、4（specified：設計 10.13.13 の 10「`stream_s` が無い記録は、今のとおり `session_start` を受けた時刻で寄せる」） | 境界、陰性 | `a_ready_without_stream_s_is_placed_by_the_session_start_as_before` |
| T-BR-13-007 | 記録 C の発話 1 の `final` を取り除き（行 1 に `final` が無い）、同じ関数に渡す | 最初の `final` が欠ける | 起点は `final` に頼らない | 発話 2 から 4 の `final` が窓 2 から 4 に 1 件ずつ割り当たる（`finals` が 0、1、1、1）。最初の `final` を起点にする実装は、発話 2 の `final` を窓 1 に置いて落ちる（specified：設計 10.13.13 の 10「最初の final を起点にする形は採らない」） | 陰性 | `the_origin_does_not_depend_on_the_first_final_of_the_connection` |
| T-BR-13-008 | `native/hayamimi/src/ingest.rs` の `mod tests`：`ready` の frame を作る関数に、流れの位置 30904000 µs を渡す | `ready` の項目 | `stream_s` は秒の数 | frame の JSON に `stream_s` が数 30.904 で在り、`type` が `ready`、`sr` が 16000 のまま。流れの位置 0 では `stream_s` が数の 0（欠けでも null でもない）（specified：設計 10.13.13 の 10。足した key は既存の key と型を変えない：`rust_port.ja.md` の RP-12） | 陽性、境界 | `the_ready_frame_carries_the_stream_position_in_seconds` |

**BR-14（同じ `id` の清書が頁で 1 行になる。`native/hayamimi-ui/src/model.rs`、実走は `native/hayamimi/tests/br14.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-14-001 | 受入：`id` が `r-1` の清書を話者 `S1?` で `apply` し、`of: refine` の訳（`r-1`、text `tr1`）を `apply` し、同じ `id` の清書を話者 `S1` で `apply` する | 訳が先に付いた後の置き換え | 1 行 | `refines` が 1 行、話者が `S1`、訳が `tr1` のまま残る（specified：BR-14「話者は後の物、訳は前後どちらに付いた物も残る」） | 陽性 | `br14_a_refined_paragraph_sent_again_with_its_id_replaces_the_first` |
| T-BR-14-002 | 同じ `id` の清書 2 件を `apply` した後に、その `id` の `of: refine` の訳を `apply` する | 訳が後に着く | 1 行 | `refines` が 1 行で、その行に訳が付く（specified：BR-14） | 陽性 | `a_translation_arriving_after_the_replacement_joins_the_single_row` |
| T-BR-14-003 | 違う `id`（`r-1`、`r-2`）の清書 2 件 | 別の `id` | 2 行 | `refines` が 2 行（specified：BR-14 の陽性対照） | 陽性 | `refined_paragraphs_of_different_ids_stay_two_rows` |
| T-BR-14-004 | 同じ内容の清書（`r-1`）を 2 回 `apply` する | 繋ぎ直しの流し直し | 1 行 | `refines` が 1 行（specified：BR-14 の陰性対照） | 陰性 | `the_same_refine_replayed_after_a_reconnect_stays_one_row` |
| T-BR-14-005 | `r-1`、`r-2`、`r-3` の清書を `apply` し、同じ 3 件をもう一度 `apply` する | 流し直しが複数件 | 3 行 | `refines` が 3 行で、順番が `r-1`、`r-2`、`r-3`（流し直しで並びが入れ替わらない）（specified：設計 10.13.5 の Q-h の 2 つ目の道） | 陰性 | `a_replay_of_several_refines_leaves_the_same_rows` |
| T-BR-14-011 | `r-1`、`r-2` の清書を `apply` し、`r-1` を text だけ変えてもう一度 `apply` する | 先頭の行の置き換え | 前の行の位置のまま | `refines` の順番が `r-1`（新しい text）、`r-2`。最後へ動かさない。`refine_total()` が 2 のまま（specified：設計 10.13.13 の 4） | 境界 | `a_replaced_row_keeps_its_place_in_the_order` |
| T-BR-14-012 | `r-1` に、前に付いた訳（`en`、text `tr1`）と後に付いた訳（`en`、text `tr2`）、別の言語の訳（`zh`、text `tz`） | 同じ言語の訳が 2 つ | 後の物 | `r-1` の行の訳が `en` は `tr2`、`zh` は `tz` の 2 つ（specified：設計 10.13.13 の 4「同じ言語の訳が 2 つ在れば後の物」。別の言語の訳は残る） | 境界 | `of_two_translations_in_one_language_the_later_one_stays` |
| T-BR-14-013 | 異なる `id` の清書 `r-1` から `r-501` を `apply` し（`r-1` は押し出され、`dropped_refines` が 1）、続けて `r-1` をもう一度 `apply` する | 置き換える前の行が既に押し出されている | 新しい行として末尾に足す | `r-1` は新しい行として末尾に足り、最も古い行（`r-2`）が 1 つ押し出される。`refines` が 500 行、`dropped_refines` が 2、末尾の行の `id` が `r-1`（specified：設計 10.13.13 の 27） | 境界 | `a_refine_whose_earlier_row_was_pushed_out_is_appended_as_a_new_row` |
| T-BR-14-006 | `id` の無い清書 2 件 | `id` が無い | 置き換えない | `refines` が 2 行（`id` が無ければ結ぶ物が無い）（derived：`id` は `Option`。`on_refine` は `found.id` を引く） | 境界 | `refines_without_an_id_are_never_replaced` |
| T-BR-14-007 | `r-1` を話者 `S1?` と text `A` で、同じ `id` を text `B` で `apply` する | 本文が変わる | 後の物 | 行の本文が `B`（specified：BR-14「話者は後の物」と同じく、後の清書で置き換える） | 境界 | `the_later_refine_replaces_the_text_as_well` |
| T-BR-14-008 | 置き換えの前後の `refine_total()` と `dropped_refines` | 置き換えは新しい行でない | 総数が増えない | 置き換えの後の `refine_total()` が前と同じ（1）。末尾へ追う動きを起こさない（derived：頁の `use_follow` が総数の増加で動く） | 境界 | `a_replacement_does_not_grow_the_refined_total` |
| T-BR-14-009 | 異なる `id` の清書を 500 件 `apply` し（`ROW_LIMIT`）、最後の `id` をもう一度 `apply` する | 上限ちょうどでの置き換え | 古い行を落とさない | `refines` が 500 行のまま、`dropped_refines` が 0（置き換えが上限の押し出しを起こさない）。501 件目の異なる `id` は `dropped_refines` が 1 になる（specified：設計 10.13.13 の 27「置き換えは行の数を変えないので、古い行を押し出さない」。後者が陽性対照） | 境界 | `a_replacement_at_the_row_limit_drops_nothing` |
| T-BR-14-010 | 実走：頁を開いたまま `lat-dialog` を流し、頁に届いた清書の `id` と話者を記録する | 実際の二重送り | 行の数 = `id` の数 | `.refine-text` の数が、届いた清書の異なる `id` の数と等しい。同じ `id` が 2 回以上届いた `id` が 1 つ以上ある run でなければ、通と読まずに「重なりが起きなかった」で落ちる（specified：BR-14。設計 10.13.10 の H57。0 件の判定を通と読まない） | 陽性 | `the_refined_rows_of_the_page_are_as_many_as_the_distinct_ids_that_arrived` |

**BR-15（`final` が覆った速報が Listening の欄に残らない。`native/hayamimi-ui/src/model.rs`、実走は `native/hayamimi/tests/br14.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-15-001 | 受入：速報 `こん` を `apply`、`final`（`a-1`）を `apply`、次の速報 `次` を `apply` | 速報、`final`、次の速報 | `final` で空 | 2 つ目の後で `strip_text` が空、3 つ目の後で `次`（specified：BR-15） | 陽性 | `br15_the_listening_strip_drops_a_partial_its_final_has_covered` |
| T-BR-15-002 | 速報 `こん` だけを `apply` | `final` が無い | 欄に出る | `strip_text` が `こん`（specified：BR-15 の陽性対照） | 陽性 | `a_partial_alone_stays_in_the_strip` |
| T-BR-15-003 | 速報の訳（`of: partial`）が付いた速報の後に `final` を `apply` | 速報の訳と欄の文 | 両方消える | `strip_text` が空で、`partial_tr` が `None`（今在る test `a_final_empties_the_partial_line_and_the_translation_of_the_partial` の末尾の `strip_text` の期待値は `こん` から空へ替える。specified：BR-15。derived：その test の今の主張） | 陽性 | `a_final_empties_the_strip_text_together_with_the_partial_translation` |
| T-BR-15-004 | 速報の無いまま `final` を `apply` | 速報が先に無い | 空のまま | `strip_text` が空で、その他の状態が `final` の分だけ動く（`finals` が 1 行） | 境界 | `a_final_with_no_partial_before_it_leaves_the_strip_empty` |
| T-BR-15-005 | 実走：頁を開いたまま `lat-dialog` を流し、最後の `final` の後の `.strip` と `.strip-text` を読む。最後の `final` の前に届いた最後の速報の text を `/events` から取る | 実際の流れの最後 | 速報の文が残らない | `.strip-text` の text が、頁の言語の `waiting` の文言（`strings.en.toml` と `strings.ja.toml`）と等しく、最後の速報の文の字を 1 つも含まず、`.strip` が class `quiet` を持つ。DOM の文字列が 0 字であることは求めない。最後の速報の text が空の run は通と読まない（specified：BR-15 の陰性対照と設計 10.13.13 の 16） | 陰性 | `the_strip_of_the_page_shows_waiting_after_the_last_final` |
| T-BR-15-006 | 実走：005 の run の最後の `final` の直前（最後の速報の後） | 速報の後で `final` の前 | 速報が出る | `.strip-text` の text が最後の速報の text と等しい（005 が読む欄が、速報の間は文を出すことを見る陽性対照） | 陽性 | `the_strip_of_the_page_shows_the_partial_before_its_final` |
| T-BR-15-007 | 純関数：発話 1 の速報 `こん` を `apply`、発話 2 の速報 `次` を `apply`、続けて発話 1 の `final`（遅れて着いた物）を `apply`。続けて発話 2 の速報 `次の` を `apply` | `final` が次の速報より遅れる | `final` は着いた時に必ず空にする | 遅れた `final` の後で `strip_text` が空（`次` も消える）、次の速報の後で `次の`（specified：設計 10.13.13 の 5。どの速報がどの `final` に覆われたかは見分けない） | 境界 | `a_late_final_empties_the_strip_and_the_next_partial_fills_it_again` |

**B4（別の PC のブラウザから音を送る頁）の行：BR-16、BR-17、BR-18、BR-35**

B4 は TLS も証明書も持たない。送る側の browser の flag（`unsafely-treat-insecure-origin-as-secure`）が 1 つの http の origin を secure context として扱い、頁 `/send` と `/ingest` は `--send-page` を付けた `--serve` の listener（http）に同居する。

**B4 の行の共通の材料**：

- 起こし方：`--send-page --serve 0 --serve-host 0.0.0.0`。`--ws-host` は既定の loopback のまま。
- LAN の address と頁の URL：loopback の相手が `GET /send-link` を打ち、返った URL を読む。URL の host が LAN の address（`hayamimi_relay::lan::lan_url` の答え）、`#t=` の後ろが合言葉である。`/send-link` が 503（経路が無い）なら、BR-16、BR-17、BR-35 の LAN の相手の行は skip でなく `Err` を返し、BR-18 は「測っていない」と記録に書く。引いた address が受ける範囲（10.13.13 の 6 の LAN）の外なら、これも `Err` を返す。
- 相手の address：LAN の address を相手にする接続は、client の socket を LAN の address に bind して `<LAN の address>:<port>` へ繋ぐ。loopback の相手は `127.0.0.1:<port>` へ繋ぐ。
- 頁の自分の origin：`/send-link` の URL の scheme、host、port から組んだ文字列（`http://<LAN の address>:<port>`。IPv6 の host は `[` と `]` で包む）。`Origin` の頭は、これと字のとおり一致する時だけ受ける。
- 「断られた」：upgrade の応答が 101 でないか、`ready` より前に close の frame を受けること。断りの `reason` は `ingest.client.refused` の行で読む。
- WebSocket の client は test が書く（`Origin` と最初の frame を行ごとに決める）。最初の frame の既定は `{"token":"<合言葉>"}` の text の frame。
- 合言葉を照らす前の接続が席を握らないこと（C2）は、`Lease::take` が呼ばれた回数でなく、外から見える `ready` と `busy` で測る。

**BR-16（`--send-page` の `/ingest` の断りと、合言葉を照らす前の接続が席を握らないこと。`native/hayamimi/tests/br16.rs`）**

address の判定は、相手の address を受けて「受ける」か `address` で断るかを返す純関数に直接渡す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-16-001 | 受入：`--serve` の listener の `/ingest` へ、(a) 最初の frame に `token` の無い接続、(b) 違う合言葉の接続、(c) 自分と別の `Origin`（`http://other.example`）の接続 | 3 つの断り | 1 接続に 1 行 | 3 本とも断られ、`ingest.client.refused` が `reason` の `token`、`token`、`origin` で 1 行ずつ出る。log の全行に合言葉の文字が無い（specified：BR-16） | 陽性 | `br16_a_lan_ingest_without_the_token_or_from_another_origin_is_refused` |
| T-BR-16-002 | 正しい合言葉と自分の origin の接続 | 受ける側 | `ready` | `ready` を受ける（specified：BR-16 の陽性対照） | 陽性 | `a_connection_with_the_token_and_its_own_origin_gets_ready` |
| T-BR-16-003 | 同じ run の localhost の 8766 の `/ingest`（`--ws-port` は OS に選ばせる）へ合言葉なしで接続 | localhost は今のまま | `ready` | `ready` を受け、`ingest.client.refused` が 0 件（specified：BR-16 の陰性対照。設計 10.13.6） | 陰性 | `the_localhost_ingest_still_needs_no_token` |
| T-BR-16-004 | 純関数に 10.0.0.0 、10.255.255.255、9.255.255.255、11.0.0.0、172.15.255.255、172.16.0.0、172.31.255.255、172.32.0.0、192.167.255.255、192.168.0.0、192.168.255.255、192.169.0.0 | RFC 1918 の 3 範囲の両端の内と外 | 範囲の端 | 10.0.0.0、10.255.255.255、172.16.0.0、172.31.255.255、192.168.0.0、192.168.255.255 を受け、残りの 6 つを `address` で断る（specified：設計 10.13.6） | 境界 | `the_three_private_ranges_are_accepted_on_both_edges_and_refused_just_outside` |
| T-BR-16-005 | 純関数に 169.253.255.255、169.254.0.0、169.254.255.255、169.255.0.0、126.255.255.255、127.0.0.0、127.255.255.255、128.0.0.0 | link-local と loopback の両端 | 範囲の端 | 169.254.0.0、169.254.255.255、127.0.0.0、127.255.255.255 を受け、残りの 4 つを断る | 境界 | `link_local_and_loopback_ranges_are_accepted_on_both_edges_and_refused_just_outside` |
| T-BR-16-006 | 純関数に `fbff:ffff:ffff:ffff:ffff:ffff:ffff:ffff`、`fc00::`、`fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff`、`fe00::`、`fe7f::`、`fe80::`、`febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff`、`fec0::`、`::1`、`::` | IPv6 の ULA、link-local、loopback の両端 | 範囲の端 | `fc00::`、`fdff:…`、`fe80::`、`febf:…`、`::1` を受け、残りの 5 つを断る | 境界 | `the_ipv6_unique_local_and_link_local_ranges_are_accepted_on_both_edges` |
| T-BR-16-007 | 純関数に 8.8.8.8、100.64.0.1、0.0.0.0、224.0.0.1、255.255.255.255、`2001:4860:4860::8888` | LAN の外のうち、公開、共有 address 空間、特殊 | どれも断る | 6 つとも `address` で断る（specified：設計 10.13.6「ほかは reason: address で断る」。陽性対照は 004 から 006 の受ける側） | 陰性 | `public_and_special_addresses_are_refused_with_reason_address` |
| T-BR-16-008 | 純関数に `::ffff:192.168.1.5`、`::ffff:8.8.8.8` | IPv4 を包んだ IPv6 | 中の IPv4 で決める | 前者を受け、後者を `address` で断る（specified：設計 10.13.13 の 6「包んだ IPv4 の address に直してから判定する」） | 境界 | `an_ipv4_address_wrapped_in_ipv6_is_judged_by_the_address_inside` |
| T-BR-16-009 | 自分の origin が `http://<LAN の address>:<port>` の時、`Origin` が (a) 無い、(b) `null`、(c) port だけ違う、(d) scheme だけ `https`、(e) host だけ `localhost`、(f) 末尾に `/` を足した物、(g) scheme だけ大文字の `HTTP://`、(h) 自分と同じ | origin の同値類 | 字のとおり一致する時だけ | (a) から (g) は断られ `reason` が `origin`、(h) だけ `ready`（specified：設計 10.13.6「頁の URL と origin」。`/send-link` の URL から組んだ 1 つの文字列と `Origin` の頭が字のとおり一致する時だけ受ける） | 境界 | `only_the_exact_own_origin_is_accepted` |
| T-BR-16-010 | 最初の JSON の frame の `token` が (a) 無い、(b) 空文字、(c) 同じ長さの違う値、(d) 正しい値の先頭 1 文字を欠いた値、(e) 正しい値の末尾に 1 文字足した値（33 字）、(f) 正しい値の英字を大文字にした値（32 字）、(g) 正しい値の先頭 1 字を `g` に替えた値（32 字で 16 進でない字を含む） | 合言葉の同値類 | 小文字の 16 進 32 字で、完全一致だけ | 7 本とも断られ `reason` が `token`（specified：BR-16 と設計 10.13.6「大文字を含む合言葉は違う合言葉として断る」。各 1 行。正しい値に英字が無い run（確率は (10/16)^32、約 3 × 10^-7）では (f) が正しい値と同じになるので、その run は (f) を測らず、024 の純関数が (f) を決める） | 境界 | `any_token_other_than_the_exact_one_is_refused` |
| T-BR-16-011 | 正しい合言葉を URL の query にだけ置き、最初の frame に `token` を置かない接続 | query の合言葉 | 採らない手段 | 閉じられ、`reason` が `token`（specified：設計 10.13.6「合言葉を URL の query に置く」を採らない物とする） | 陰性 | `a_token_in_the_query_string_is_not_accepted` |
| T-BR-16-012 | 001 から 011 の run の stderr と log の全行、`GET /events` を開いたままにして受けた全 event、断りの close の frame の reason の文字で、正しい合言葉の文字列を探す。陽性対照として、`GET /send-link` の応答の body で同じ文字列を探す | 合言葉が出ない所 | 0 件 | stderr、log、`/events`、close の reason は 0 件、`/send-link` の body は 1 件（specified：設計 10.13.6「合言葉は `/events`、log、error の文、中継のどれにも出さない」と BR-16。0 件は非 0 の対照が付いて初めて読む。中継へ出ないことは test が中継を起こさないので測らず、命令で測れない述語の一覧に置いた） | 陰性 | `the_token_never_reaches_the_log` |
| T-BR-16-013 | 違う合言葉で断られた接続に、断られた後で音声の frame（16 kHz、mono、`pcm_s16le` の 100 ms）を送る | 断った後 | 受けない | `ready` は届かず、frame の送信は失敗するか無視され、`ingest.client.connected` が 0 行（specified：BR-16「どれも閉じられ」。陽性対照は 002 の `ready` と、正しい合言葉の接続で `ingest.client.connected` が 1 行出ること） | 陰性 | `a_refused_connection_gets_no_ready_and_no_final` |
| T-BR-16-014 | 正しい合言葉と自分の origin の 1 本目が音を送っている間に、同じ条件の 2 本目を `--serve` の listener の `/ingest` へ接続する | 2 本目の client | 1013 で閉じる | 2 本目は `ready` を受けず WebSocket の close code 1013 で閉じられ、`ingest.client.refused` が `reason: busy` で 1 行出る。1 本目は閉じられず（close の frame を受けず）、2 本目が閉じられた後の音の frame の送信が成功する（specified：設計 10.13.13 の 8。`final` は model を要るのでこの行は読まない） | 陰性 | `a_second_client_while_one_is_sending_is_closed_with_1013_and_busy` |
| T-BR-16-015 | 同じ入れ方を localhost の `/ingest`（port は OS に選ばせる）で | localhost も同じ 1 本の制限 | 1013 で閉じる | 014 と同じ（`reason: busy`、close code 1013、1 本目は止まらない）（specified：設計 10.13.13 の 8「`--send-page` で足した `/ingest` でも localhost の 8766 でも」） | 陰性 | `a_second_localhost_client_is_closed_with_1013_and_busy` |
| T-BR-16-016 | 受入：`--serve` の listener の `/ingest` へ、upgrade の後に何も送らない接続 A を開いたままにし、A の upgrade の 1 秒後に、正しい合言葉と自分の origin の接続 B を送る | 合言葉を照らす前の接続と席 | B は A を待たない | B が `ready` を受け、`busy` で断られない。A は upgrade から 3 秒以内（計り方の許容は 1 秒で、4 秒以内）に close code 1008 で閉じられ、`ingest.client.refused` が `reason: token` で 1 行出る。B の `ready` は A が閉じられる前に届く（specified：設計 10.13.6 の C2「合言葉を、音の席を取る前に照らす」。陽性対照は 002 の B 単独の `ready`） | 陰性 | `br16_a_silent_connection_does_not_hold_the_seat` |
| T-BR-16-017 | 最初の frame（正しい合言葉）を upgrade の (a) 2.0 秒後、(b) 4.0 秒後に送る | 3 秒の期限の両側 | 3 秒 | (a) は `ready`、(b) は送る前に A と同じく close code 1008 で閉じられ（`reason: token`）、frame を送っても `ready` は届かない。3.0 秒ちょうどは測らない（時計の揺れで通と落ちが入れ替わるため）（specified：設計 10.13.6 の C2「upgrade から 3 秒以内」） | 境界 | `the_first_frame_must_arrive_within_three_seconds` |
| T-BR-16-018 | 最初の text の frame を、正しい合言葉の JSON の後ろに空白を足して、(a) ちょうど 4096 byte、(b) 4097 byte にして送る | 4096 byte の両側 | 4096 byte 以下 | (a) は `ready`、(b) は close code 1008 で閉じられ `reason: token`（specified：設計 10.13.6 の C2「4096 byte 以下の text の frame」。derived：JSON の後ろの空白は読み取りで無視される） | 境界 | `the_first_frame_of_4096_bytes_is_accepted_and_of_4097_is_refused` |
| T-BR-16-019 | 最初の frame を、正しい合言葉の JSON の中身を持つ binary の frame にして送る | text でない最初の frame | text だけ | close code 1008 で閉じられ `reason: token`（specified：設計 10.13.6 の C2「text の frame でなければならず」。陽性対照は 018 の (a)） | 陰性 | `a_binary_first_frame_is_refused` |
| T-BR-16-020 | 合言葉を照らす前の接続（upgrade の後に何も送らない）を 8 本開き、続けて 9 本目の upgrade を送る | 8 本の両側 | 同時に 8 本 | 8 本とも 101 を受け、9 本目は 101 を受けず、HTTP の status が 503 で `Retry-After: 3` の頭を持ち、`ingest.client.refused` が `reason: pending_full` で 1 行出る（specified：設計 10.13.6 の C2「9 本目は upgrade の前に HTTP の 503（`Retry-After: 3`）で断り」） | 境界 | `the_ninth_unchecked_connection_is_refused_before_the_upgrade` |
| T-BR-16-021 | 020 の 8 本が期限（3 秒）で閉じられた後に、正しい合言葉と自分の origin の接続を送る | 枠が戻る | 8 本の枠が空く | `ready` を受ける（`busy` でなく、503 でもない）（specified：設計 10.13.6 の C2。020 の 9 本目の断りが「枠が戻らない実装」でなく「同時の上限」であることの対照） | 陽性 | `the_unchecked_connection_slots_return_after_the_deadline` |
| T-BR-16-022 | 1 本目（正しい合言葉）が音を送っている間に、2 本目が (a) 違う合言葉、(b) 正しい合言葉で、どちらも自分の origin | 照らす順 | 合言葉が先、席が後 | (a) は `reason: token`（`busy` でない）、(b) は `reason: busy` と close code 1013。どちらでも 1 本目は閉じられない（specified：設計 10.13.6 の C2「照らし終えた接続だけが席を取りに行き、席が埋まっていれば今のとおり `busy`」。(b) が (a) の対照） | 境界 | `the_token_is_checked_before_the_seat_is_asked_for` |
| T-BR-16-023 | 違う合言葉で断られた接続の直後に、正しい合言葉と自分の origin の接続を送る | 断りの後の席 | 席は空いたまま | `ready` を受ける（`busy` でない）。断られた接続が席を取っていない（specified：設計 10.13.6 の C2「席を取らずに閉じる」。陽性対照は 002） | 陰性 | `a_refused_connection_leaves_the_seat_free` |
| T-BR-16-024 | 純関数：合言葉を照らす関数に、正しい値 `0123456789abcdef0123456789abcdef` に対して、同じ値、`0123456789ABCDEF0123456789ABCDEF`、`0123456789abcdef0123456789abcde`（31 字）、`0123456789abcdef0123456789abcdef0`（33 字）、`g123456789abcdef0123456789abcdef`、`0123456789abcdef0123456789abcdee`（最後の 1 字だけ違う）、空文字 | 形と値の同値類 | 小文字の 16 進 32 字で完全一致 | 同じ値だけ受け、残りの 6 つを `token` で断る（specified：設計 10.13.6「字の数が 32 で、どの字も `0`〜`9` と `a`〜`f` である物だけを受け…大文字を含む合言葉は違う合言葉として断る」。比べ方（`subtle` の `ConstantTimeEq`）は時間を測れないので、perfectionist-reviewer が `rg -n 'ConstantTimeEq' native/hayamimi/src` が 1 件以上であることと、合言葉の比較に `==` を使っていないことをコードの読みで確かめる） | 境界 | `only_a_lowercase_hex_token_of_32_characters_that_matches_is_accepted` |

**BR-17（`/send` と `/send-link` の出し方と、起動の組み合わせの止め。`native/hayamimi/tests/br17.rs`）**

材料：共通の材料（BR-16 の前）に従う。「起動が止まる」は、`hayamimi` を起こした process が 10 秒以内に 0 でない exit code で終わり、起動してから終わるまでの間、`--serve` の port（test が空きを引いて渡す）への TCP 接続を 50 ms ごとに試して 1 度も成功しないことで測る。「起動する」は、10 秒以内に `GET /dashboard` が 200 になり、process がまだ生きていることで測る。`/send-link` の応答の URL は `http://<LAN の address>:<port>/send#t=<合言葉>` の形である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-17-001 | 受入：(a) `--send-page` を付けて起こし、`GET /send` と、loopback の相手の `Host: 127.0.0.1:<port>` の `GET /send-link`。(b) `--send-page` 無しで起こし、同じ 2 つ | 配る、配らない | 200 と 404 | (a) の `/send` が 200 で content type が html、body に `<html` と `<title` が在る。`/send-link` が 200 で、body が `http://<LAN の address>:<port>/send#t=<合言葉>` の形の URL。(b) は 2 つとも 404（specified：BR-17）。`/send-link` が 503 の機械は共通の材料の扱い | 陽性 | `br17_the_send_page_and_the_send_link_are_served_only_with_the_flag` |
| T-BR-17-002 | 001 の (a) の `GET /dashboard` | 同じ listener | 200 | 200（specified：BR-17 の陽性対照） | 陽性 | `the_send_page_run_still_serves_the_dashboard` |
| T-BR-17-003 | `--send-page` 無しで起こした `--serve` の listener の `/ingest` へ、WebSocket の upgrade を送る | `/ingest` を足さない側 | 足さない | 101 を受けず、404（specified：設計 10.13.6「`--send-page` を付けた時だけ、`/send` の頁と `/ingest` を足す」。陽性対照は BR-16 の 002 が `--send-page` の run で `ready` を受けること） | 陰性 | `the_serve_listener_has_no_ingest_without_the_send_page_flag` |
| T-BR-17-004 | loopback の相手が `GET /send-link` を `Host` が (a) `127.0.0.1:<port>`、(b) `[::1]:<port>`、(c) `localhost:<port>` で打つ | loopback の名前の 3 つ | 3 つとも受ける | 3 本とも 200 で、body が同じ URL（`http://<LAN の address>:<port>/send#t=<合言葉>`）（specified：設計 10.13.6 の C3。(b) は `[::1]` を名乗るだけで、接続は `127.0.0.1` へ張る） | 陽性 | `the_send_link_answers_the_three_loopback_host_names` |
| T-BR-17-005 | loopback の相手が `GET /send-link` を `Host` が (a) `attacker.example`、(b) `127.0.0.1.attacker.example:<port>`、(c) `localhost.attacker.example:<port>`、(d) `[::1].attacker.example:<port>` で打つ | loopback の名前に似た外の名前 | 名前が完全に loopback の名前の時だけ | 4 本とも 404（specified：BR-17 の陰性対照と設計 10.13.6 の C3。陽性対照は 004） | 陰性 | `a_foreign_host_name_gets_no_send_link_even_from_loopback` |
| T-BR-17-006 | LAN の address の相手（LAN の address に bind した client）が `GET /send-link` を `Host` が (a) `127.0.0.1:<port>`、(b) `<LAN の address>:<port>` で打つ | loopback でない相手 | 受けない | 2 本とも 404（specified：BR-17 の陰性対照。陽性対照は 004） | 陰性 | `a_send_link_request_from_a_lan_address_is_not_answered` |
| T-BR-17-007 | 004 の (a) の応答と、005 の (a) の 404 の応答の頭 | 応答の頭 | 付ける、付けない | 200 の応答に `Cache-Control: no-store` が在り、200 と 404 のどちらにも `access-control-allow-origin` が無い。`Origin: http://attacker.example` を付けて打っても同じ（specified：設計 10.13.6 の C3） | 境界 | `the_send_link_carries_no_store_and_no_cors_header` |
| T-BR-17-008 | `GET /send-link` を同じ run で 2 回、別の run（再起動）で 1 回 | 合言葉の寿命 | 起動ごと | 同じ run の 2 回は同じ合言葉、別の run は違う合言葉。合言葉はどの run でも、字の数がちょうど 32 で、どの字も `0`〜`9` か `a`〜`f`（正規表現 `^[0-9a-f]{32}$` に一致する）（specified：設計 10.13.6「16 byte の合言葉を小文字の 16 進の 32 字に書く」） | 境界 | `the_token_is_fixed_in_a_run_and_new_in_the_next` |
| T-BR-17-009 | `--send-page --serve-host 0.0.0.0` に `--ws-host` が (a) `0.0.0.0`、(b) 引いた LAN の address、(c) `::` を重ねて起こす | loopback でない `--ws-host` | 止まる | 3 本とも起動が止まり、`--serve` の listener も開かない。stderr に `--send-page` と `--ws-host` の両方の語が出る（specified：BR-17 と設計 10.13.6 の C5。derived：「理由を出す」を、両方の旗の名が出ることで測る） | 陰性 | `send_page_with_a_non_loopback_ws_host_stops_before_any_listener_opens` |
| T-BR-17-010 | `--send-page --serve-host 0.0.0.0` に `--ws-host` が (a) `127.0.0.1`、(b) `::1`、(c) 無し（既定）を重ねて起こす | loopback の `--ws-host` | 起動する | 3 本とも起動する（specified：設計 10.13.6 の C5「loopback でない時」だけ止める。009 の対照） | 陽性 | `send_page_with_a_loopback_ws_host_starts` |
| T-BR-17-011 | `--send-page` を付けずに `--ws-host 0.0.0.0` を付けて起こす | C5 は `--send-page` の時だけ | 今のまま | 起動する（specified：設計 10.13.6 の C5「`--send-page` に、loopback でない `--ws-host` が重なった時」。009 の対照） | 境界 | `a_non_loopback_ws_host_without_the_send_page_flag_still_starts` |
| T-BR-17-012 | `--send-page --tls-cert x.pem --tls-key y.pem` で起こす | 作らない旗 | 旗が無い | 起動が止まり、exit code が 0 でなく、stderr に知らない引数の旨が出る（specified：設計 10.13.6「TLS の listener、`--tls-cert`、`--tls-key` は作らない」。陽性対照は 001 の (a)） | 陰性 | `the_tls_flags_are_not_accepted` |
| T-BR-17-013 | `--send-page` に `--serve-host` が (a) 無し（既定）、(b) `127.0.0.1`、(c) `::1`、(d) `127.0.0.2` を重ねて起こす | loopback の `--serve-host` | 止まる | 4 本とも起動が止まり、exit code が 0 でなく、`--serve` の listener も開かない。stderr に `--send-page` と `--serve-host` の両方の語が出る（specified：BR-17 と設計 10.13.6「`--send-page` に loopback の `--serve-host`（既定の `127.0.0.1` を含む）を組み合わせた起動は、起動を止めて理由を出し、exit code を 0 でなくする」。陽性対照は 001 の (a) の `0.0.0.0`） | 陰性 | `send_page_with_a_loopback_serve_host_stops_before_any_listener_opens` |
| T-BR-17-014 | `--send-page --serve-host 0.0.0.0` の run の `GET /send-link` の URL の host | 全部の interface に bind した時 | `lan_url` の答え | host が `hayamimi_relay::lan::lan_url` に `0.0.0.0` の socket の address を渡して返る address（この PC が network へ送り出す address）と等しく、`0.0.0.0` でも loopback でもない（specified：BR-17 と設計 10.13.6「頁の URL と origin」。1 つの address に bind した時はその address になることは 015 の純関数が測る：その address に bind した run には loopback の相手が届かず、`/send-link` を打てないため） | 陽性 | `the_send_link_url_host_is_the_lan_url_address` |
| T-BR-17-015 | 純関数：`/send-link` の応答を組む関数に、`lan_url` の答えが (a) 既知の address、(b) `Unknown`。続けて URL の host を決める関数に、bind した socket の address が `192.168.1.5`（1 つの address）のとき | 経路の有無と bind の形 | 503 | (a) は 200 と `http://<address>:<port>/send#t=<合言葉>`、bind が `192.168.1.5` なら host も `192.168.1.5`。(b) は 503 で body に URL と合言葉を含まず、warn の log が 1 行（specified：設計 10.13.6「`lan_url` が `Unknown` を返す時は、`/send-link` は URL を返さずに 503 を返し、warn を 1 行出す」。この関数の置き場は `/send-link` を組む module の `mod tests` で、実装の席が決める） | 境界 | `the_send_link_is_503_without_a_route_and_warns_once` |
| T-BR-17-016 | 純関数：頁の origin を組む関数に、(a) host `192.168.1.5`・port 8080、(b) host `fd00::5`・port 8080 | host の形 | 字のとおり | (a) は `http://192.168.1.5:8080`、(b) は `http://[fd00::5]:8080`（specified：設計 10.13.6「IPv6 の host は `[` と `]` で包む」。001 の URL から組んだ origin が BR-16-002 で受けられることが実走の対照） | 境界 | `the_page_origin_wraps_an_ipv6_host_in_brackets` |
| T-BR-17-017 | 実走：`--send-page --serve-host 0.0.0.0` の run の `http://127.0.0.1:<port>/dashboard` を `chrome-headless-shell` で開き、頁の読み込みと `/send-link` の応答（`Network.responseReceived`）が済んで 2 秒後に `document.body.innerText` を読む。`/send-link` の応答の body を別に読んで URL と origin を引く | loopback で開いた dashboard | 2 つが並んで出る | `innerText` に、`/send-link` の URL（`http://<LAN の address>:<port>/send#t=<合言葉>`）の全文と、flag に登録する origin の文字列（`http://<LAN の address>:<port>`）の全文が、どちらも 1 件以上含まれ、URL の出る位置と origin の出る位置の差が `innerText` の中で 1000 字以内（並べて出す）。頁の文言と配置は designer の板が決めるので、見つける字は URL と origin の文字列だけで、要素の名は問わない（specified：設計 10.13.6「頁の URL と origin」「`/dashboard` は `/send-link` の URL と並べて、flag に登録する origin の文字列を出す」。対照は 018 と、`--send-page` 無しの run の同じ頁に 2 つとも出ないこと） | 陽性 | `br17_the_dashboard_opened_on_loopback_shows_the_send_link_and_the_origin` |
| T-BR-17-018 | 実走：017 の run の `http://<LAN の address>:<port>/dashboard` を `chrome-headless-shell` で開く（host が LAN の address なので、相手の address は LAN の address になる）。同じ待ちの後に `innerText` を読む。続けて、`--send-page` 無しの run の `http://127.0.0.1:<port>/dashboard` も同じに読む | LAN の相手が開いた dashboard、旗の無い run | どちらも出ない | 2 つの頁とも、`innerText` に合言葉の文字（`#t=` に続く 32 字）と `/send#t=` が無く、origin の文字列（`http://<LAN の address>:<port>`）も無い（specified：設計 10.13.6「相手の address が loopback の時だけ…ほかは 404」と「`--send-page` を付けた時だけ」。0 件は 017 の 1 件以上が対照） | 陰性 | `the_dashboard_opened_from_the_lan_shows_neither_the_send_link_nor_the_origin` |

**BR-18（別のブラウザの頁から音が届く。記録。`native/hayamimi/tests/br18.rs`）**

材料：`hayamimi` は共通の材料の起こし方に、model と `--input ws` を足して起こす。開く URL は `/send-link` の応答で、loopback の address（`127.0.0.0/8` は初めから secure context なので旗の効きを示さない）は使わない。偽の音の device の旗は `--use-fake-device-for-media-stream`、`--use-fake-ui-for-media-stream`、`--use-file-for-fake-audio-capture`（`lat-dialog` の行 7 の wav）。登録する旗は `--unsafely-treat-insecure-origin-as-secure=<頁の origin>`（共通の材料の文字列）。`/send-link` が 503 の機械は「測っていない」と記録に書く。旗が `chrome-headless-shell` で効かない時（設計 10.13.10 の H54）は、落とさず「旗が効かない」と記録に書き、手の試しに回す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-18-001 | 受入、実走：偽の音の device と、`/send-link` が返した URL の origin を登録した旗を渡した `chrome-headless-shell` で、`/send-link` が返した URL をそのまま開き、「マイク」を押す | 偽の device の音が届く | 記録だけ | 頁の `isSecureContext` が true。`final` が 1 件以上届き、その `lang` が `ja`。頁を開いた後の `location.hash` が空で、`location.href` に合言葉の文字が無い（`history.replaceState`）。`getDisplayMedia` の道は測らない（specified：BR-18。設計 10.13.6 の頁の中身） | 陽性 | `br18_a_browser_page_sends_its_microphone_to_ingest` |
| T-BR-18-002 | 同じ wav を `hayamimi-eval stream` で localhost の 8766 の `/ingest` へ送る | 頁を通らない経路 | `final` が出る | `final` が 1 件以上（specified：BR-18 の陽性対照。001 の 0 件が「頁が送れなかった」のでなく「`hayamimi` が読めなかった」のでないことを分ける） | 陽性 | `the_same_audio_sent_by_the_stream_tool_gives_a_final` |
| T-BR-18-003 | 登録の旗を渡して、`/send-link` の URL から `#t=<合言葉>` を消した URL で開き、「マイク」を押す | 合言葉が無い | 届かない | `final` が 0 件で、`ingest.client.refused` が 1 件（`reason` が `token`）（specified：BR-18 の陰性対照。001 の `final` が対照） | 陰性 | `the_page_without_the_token_gets_refused_and_no_final_follows` |
| T-BR-18-004 | 登録の旗を渡さない同じ `chrome-headless-shell` で、001 と同じ URL を開いて「マイク」を押す | 旗の有無 | 旗が無ければ secure context でない | 頁の `isSecureContext` が false で、`final` が 0 件（specified：BR-18 の陰性対照「同じ LAN の URL を flag 無しで開くと `isSecureContext` が false で、`final` が 0 件」。001 の `isSecureContext` true と `final` が対照。旗が効かない時は 001 と同じく「旗が効かない」と記録に書き、この行も同じ扱い） | 陰性 | `the_page_without_the_flag_cannot_take_the_microphone` |
| T-BR-18-005 | 001 の run の `Network.requestWillBeSent` が見た全要求の URL | 合言葉を URL に載せない | 0 件 | 全 URL（WebSocket の `ws://<LAN の address>:<port>/ingest` を含む）に合言葉の文字が無い。陽性対照として、001 を開いた URL（`/send-link` の応答）の文字列に合言葉が 1 件在る（specified：設計 10.13.6「合言葉を URL の query に置く」を採らない物とする。fragment はブラウザが送らない） | 陰性 | `the_token_never_appears_in_a_request_url_of_the_page` |

**頁の 3 つの枠と字体の行（BR-19 から BR-25）の共通**：窓は `Emulation.setDeviceMetricsOverride` で幅と高さを置き、`localStorage` は頁を開く前に `Page.addScriptToEvaluateOnNewDocument` で置く（空の頁は何も置かない）。「純関数」の行は `native/hayamimi-ui/src/dashboard.rs` の `mod tests` に置き、ブラウザを動かさない。「境目の割合」は縦の線では左の列の割合、横の線では上の列の割合である。左の列の幅は縦の線の `left` から `.main` の `left` を引いた値、右の列の幅は `.main` の `right` から縦の線の `right` を引いた値、清書の欄の高さは `#refined` を含む `section` の高さで測る。

**BR-19（広い頁の 3 つの枠の置き場。`native/hayamimi/tests/br19.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-19-001 | 受入、実走：窓 1280 × 800、`localStorage` が空の頁に `lat-dialog` の行 7 から 10 を流す | 既定の向き | 縦の線を境に左右 | `lang-badge` の text が `en` のどの `.row` も `right` が縦の線の `left` 以下、`ja` のどの `.row` も `left` が縦の線の `right` 以上。`#refined` を含む `section` の `left` が `.main` の `left` + 1 以下で `right` が `.main` の `right` − 1 以上、`top` が横の線の `top` 以上。`.strip` の `top` が `.main` の `top` 以下（specified：BR-19） | 陽性 | `br19_by_default_english_lines_sit_left_japanese_lines_right_and_refined_spans_both_below` |
| T-BR-19-002 | 純関数：行の側を決める関数に `ja`、`en` | 組の 2 つの言語 | `ja` は日本語の側 | `ja` は日本語の側、`en` は英語の側（specified：BR-19。B7 の後は 10.13.12 の側の読み替えが同じ結果を返す） | 陽性 | `a_ja_line_is_on_the_japanese_side_and_an_en_line_on_the_english_side` |
| T-BR-19-003 | 純関数：同じ関数に `zh`、`ko`、空の文字列 | 組の外の言語、空 | 日本語の外は英語の側 | 3 つとも英語の側（specified：BR-19「`en` と `zh` を英語の側」と設計 10.13.11「`ja` 以外は英語の側」。空は陰性対照の「`lang` の値が空の合成の行は左」） | 境界、陰性 | `any_language_but_ja_and_the_empty_language_are_on_the_english_side` |
| T-BR-19-004 | 純関数：向きを決める関数に、保存された向きが無い入力 | 既定の向き | 英語の側が左 | 英語の側が左（specified：BR-19） | 陽性 | `with_nothing_remembered_the_english_side_is_on_the_left` |
| T-BR-19-005 | 実走：001 の頁の `ja` の行のうち、英訳の `.tr` を持つ行 | 訳の言語は行の置き場を動かさない | 右のまま | 英訳の `.tr` を持つ `ja` の行の `left` が縦の線の `right` 以上（specified：BR-19 の陰性対照。英訳の `.tr` が 1 つ以上在る run だけを通と読む） | 陰性 | `a_japanese_row_with_an_english_translation_stays_on_the_right` |
| T-BR-19-006 | 実走：001 の頁の各 `.row` と、その中の `.tr` | 原文とその訳が裂かれない | 同じ塊 | どの `.tr` も、その `.row` の矩形の内側（上下左右とも ± 2 px）に在る（specified：設計 10.13.11「各行は原文と、その下の訳を 1 つの塊として持つ」） | 陽性 | `a_row_keeps_its_translation_inside_its_own_box` |
| T-BR-19-007 | 実走：001 の頁の `.row` の DOM の並びと、届いた `final` の `id` の並び | 時刻の順 | 同じ順 | `.row` の並びが届いた `final` の順（`ja`、`en`、`ja`、`en`）で、側ごとにまとめた並びでない（specified：BR-19「`row` の並びの順は届いた `final` の順」） | 陽性 | `rows_keep_the_order_the_finals_arrived_in_and_are_not_grouped_by_side` |
| T-BR-19-008 | 実走：001 の頁の `.row` の `top` と `bottom` | 1 行が 1 段 | 重ならない | 時刻の順に並べた隣の 2 行で、後の行の `top` が前の行の `bottom` − 1 px 以上（specified：設計 10.13.11「1 行が 1 段を取る」。左右の行が同じ高さに並ぶ形を落とす） | 境界 | `each_row_takes_its_own_band_even_when_the_sides_alternate` |
| T-BR-19-009 | 実走：001 の頁の `.strip` | Listening の欄 | 2 つにまたがる | `.strip` の `left` が `.main` の `left` + 1 以下で `right` が `.main` の `right` − 1 以上、`top` が `.main` の `top` 以下（specified：設計 10.13.11「2 つにまたがる幅で 1 本」） | 陽性 | `the_listening_strip_spans_both_columns_above_them` |
| T-BR-19-010 | 実走：001 の頁の `.row` を数える | 数え方の陽性対照 | 4 行 | `.row` が 4 つ、`en` が 2 つで左、`ja` が 2 つで右（specified：BR-19 の陽性対照。0 行の頁を通と読まない） | 陽性 | `the_page_has_four_rows_two_on_each_side` |

**BR-20（狭い頁の並べ替え。`native/hayamimi/tests/br20.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-20-001 | 受入、実走：窓 600 × 900 の頁に `lat-dialog` の行 7 から 10 を流す | 狭い画面 | 1 列 | どの `.row` も `left` が同じ（± 1 px）。縦の線は幅 0 か `display: none`。`.row` は届いた順に上から並び、`#refined` の `section` の `top` が `#feed` の `bottom` − 1 以上。横の線は `display` が `none` でなく、幅と高さが 0 でない（specified：BR-20） | 陽性 | `br20_a_narrow_page_stacks_the_lines_in_spoken_order_above_refined` |
| T-BR-20-002 | 実走：001 の頁を 1280 × 800 に広げ直す | 広い画面へ戻す | BR-19 の置き場 | BR-19 の 001 と同じ述語が成り立つ（specified：BR-20 の陽性対照） | 陽性 | `widening_the_narrow_page_returns_the_wide_placement` |
| T-BR-20-003 | 道具：左端が 0 の `ja` の行と 300 の `en` の行の矩形の組を、001 の「左端が同じ」の述語へ | 述語が落とす入力 | 左端の差 | 述語が `Err`（specified：BR-20 の陰性対照。述語自身が壊れていないことを、実走の外で見る） | 陰性 | `the_same_left_edge_predicate_fails_on_rows_with_different_left_edges` |
| T-BR-20-004 | 実走：窓の幅 860 px | 境の下側 | 860 以下は 1 列 | 縦の線が見えず、どの `.row` も `left` が同じ（specified：設計 10.13.11「幅 860 px 以下」） | 境界 | `a_page_860_px_wide_is_one_column` |
| T-BR-20-005 | 実走：窓の幅 861 px | 境の上側 | 861 は 2 列 | 縦の線が見え、`en` の行が左、`ja` の行が右 | 境界 | `a_page_861_px_wide_is_two_columns` |
| T-BR-20-006 | 実走：窓 1280 × 800 で縦の線を動かして左の列を 30 にし、入れ替えて、600 × 900 に狭め、1280 × 800 に戻す | 覚えた割合と向き | 捨てない | `localStorage` の `hayamimi.dashboard.split` が窓の変更の前後で同じ文字列で、戻した後の割合が 30（± 1）、向きが入れ替えた向き（specified：設計 10.13.11「狭い画面…覚えた割合と向きは捨てずに残す」） | 陽性 | `narrowing_and_widening_the_page_keeps_the_remembered_split_and_side` |

**BR-21（境目の drag と key。`native/hayamimi/tests/br21.rs`）**

純関数の材料：割合を詰める関数は「割合、全体の長さ（px）、2 つの最小（px）」を受けて、割合を最小で挟んで返す。全体が 2 つの最小の和に満たない時は既定の比を返す。key の関数は「割合、key、Shift の有無、全体の長さ、2 つの最小」を受ける。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-21-001 | 受入、実走：窓 1280 × 800。縦の線を `Input.dispatchMouseEvent` で右へ 200 px drag。続けて縦の線に focus を置いて右の矢印、Shift と右の矢印。続けて右へ 2000 px drag、横の線を下へ 2000 px drag、End の key | 動かす、最小で止まる | 左の列の幅 + 200 ± 2、最小 240 と 120 | 200 px の drag で左の列の幅が 200 ± 2 px 広がり、`aria-valuenow` が増える。右の矢印 1 回で 2 ポイント、Shift と右の矢印で 10 ポイント増える。右へ 2000 px で右の列の幅が 239.5 以上 240.5 以下、下へ 2000 px で清書の欄の高さが 119.5 以上 120.5 以下。End の key でも同じ線。覚えて読む割合と `aria-valuenow` は、収めた後の px の割合を最も近い整数へ丸めた値（specified：BR-21 と設計 10.13.13 の 18「最小の px から 0.5 px 小さい値まで許す。drag と key の結果は収めた後の px の割合の丸めを書く」。上限を 0.5 px に締めるのは、動かない実装を落とすため。200 px の drag の ± 2 px は今のまま） | 陽性 | `br21_dragging_or_keying_a_divider_resizes_the_panes_but_not_below_their_minimum` |
| T-BR-21-002 | 純関数：描く大きさを返す関数に、全体 1000 px、最小 240 と 240、覚えた割合 50、10、24、25、75、76、77、90、100、0 | 最小の線の両側 | 左の列 240 と 760 px | 左の列の px が、50 は 500、24 は 240、25 は 250、75 は 750、76 は 760、10 と 0 は 240、77 と 90 と 100 は 760。`aria-valuenow` は描いた px の割合の丸めで、50、24、25、75、76、24、24、76、76、76（specified：BR-21 と設計 10.13.13 の 18。240 / 1000 = 24 %） | 境界 | `a_split_below_the_minimum_is_pulled_to_it_on_both_ends` |
| T-BR-21-003 | 純関数：描く大きさを返す関数に、全体 600 px、最小 160 と 120（上の列と清書の欄）、覚えた割合 26、27、80、81 | 横の線の最小 | 160 / 600 = 26.67 %、480 / 600 = 80 % | 上の列の px が、26 は 160（156 px を最小へ収める）、27 は 162、80 は 480、81 は 480（486 px を上限 480 へ収める）。`aria-valuenow` は 27、27、80、80（160 / 600 = 26.67 の丸めが 27）（specified：設計 10.13.13 の 18） | 境界 | `a_horizontal_split_keeps_the_top_pane_160_and_the_refined_pane_120` |
| T-BR-21-004 | 純関数：縦の線の全体 479 px と 480 px、最小 240 と 240、割合 70 | 2 つの最小が入らない | 480 未満は既定の比 | 479 は既定の 50、480 は 50（480 では 50 だけが両方の最小を満たす）（specified：設計 10.13.11「両方の最小が入らない時は、割合を既定の比へ戻して描く」） | 境界 | `a_vertical_split_that_cannot_fit_both_minimums_falls_to_fifty` |
| T-BR-21-005 | 純関数：描く大きさを px で返す関数に、横の線の全体 279、280、281、300 px、最小 160 と 120、覚えた割合 90 と 10 | 全体が最小の和の 1 px 下、ちょうど、1 px 上、上 | 280 未満は既定の比 | 279 は覚えた割合によらず既定の 65（上の列 181.35 px）。280 は 90 でも 10 でも上の列 160 px、清書の欄 120 px（範囲 `[160, 160]`）で、`aria-valuenow` が 57（160 / 280 = 57.14 の丸め）。281 は 90 で上の列 161 px（範囲 `[160, 161]` の上端）、10 で 160 px。`aria-valuenow` はどちらも 57（57.3 と 56.9 の丸め）。300 の 90 は上の列 180 px（清書の欄 120 px）、10 は 160 px。どの行も覚えた割合は書き換えない（specified：設計 10.13.13 の 18「片方の枠を `[最小, 全体 − 他方の最小]` に収めて描き、整数の割合は覚える値と `aria-valuenow` にだけ使う」と 10.13.11「全体が最小の和より小さい時は既定の比」。derived：手計算） | 境界 | `a_horizontal_split_that_cannot_fit_both_minimums_falls_to_sixty_five` |
| T-BR-21-006 | 純関数：key の関数。割合 50 に右の矢印、Shift と右の矢印、左の矢印、Shift と左の矢印（全体 1000 px、最小 240） | key の歩幅 | 2 と 10 | 52、60、48、40。割合 75 に右の矢印は 76（77 を最小で寄せた値）、割合 25 に左の矢印は 24 | 境界 | `an_arrow_moves_two_points_and_shift_arrow_ten_inside_the_minimums` |
| T-BR-21-007 | 純関数：key の関数に Home と End（全体 1000 px、最小 240） | 端の key | 最小と最大 | Home は 24、End は 76 | 境界 | `home_and_end_move_a_divider_to_its_minimum_and_maximum` |
| T-BR-21-008 | 純関数：縦の線に上下の矢印、横の線に左右の矢印 | 向きの違う key | 動かさない | 割合が変わらない（specified：設計 10.13.11「縦の線は左右の矢印、横の線は上下の矢印」。陽性対照は 006 の向きの合う key） | 陰性 | `an_arrow_across_the_divider_does_nothing` |
| T-BR-21-009 | 純関数：drag の量を割合へ直す関数に、全体 1000 px、割合 50、右へ 200 px、左へ 200 px | pointer の量 | 200 / 1000 = 20 ポイント | 70 と 30 | 陽性 | `a_drag_of_two_hundred_pixels_in_a_thousand_moves_twenty_points` |
| T-BR-21-013 | 純関数：全体 1000 px、割合 50 から、右へ 204、205 px、左へ 204、205 px | 丸め | 最も近い整数、.5 は大きい方 | 70、71、30、30（20.4 → 20、20.5 → 21、29.6 → 30、29.5 → 30。.5 は大きい方へ）。返す値はどれも整数（specified：設計 10.13.13 の 18） | 境界 | `a_drag_ratio_is_rounded_to_the_nearest_integer_with_halves_going_up` |
| T-BR-21-010 | 実走：左へ 2000 px の drag。Home の key | 左の最小 | 240 | 左の列の幅が 239.5 以上 240.5 以下。Home でも同じ（specified：BR-21 の陰性対照と設計 10.13.13 の 18。描く大きさは px で収めるので、最小から離れない） | 陰性 | `dragging_two_thousand_pixels_left_stops_at_the_left_minimum` |
| T-BR-21-011 | 実走：001 の drag の前後 | 割合と幅の比 | 一致 | `aria-valuenow` と、左の列の幅 / (左の列の幅 + 右の列の幅) × 100 の差が 1 ポイント以内（specified：BR-21 の陽性対照） | 陽性 | `the_aria_value_agrees_with_the_widths_before_and_after_a_drag` |
| T-BR-21-012 | 実走：mouse を離した後に mouse を動かす | drag の終わり | 動かない | 離した後の動きで左の列の幅が変わらない（specified：設計 10.13.11「離した時に覚える」と pointer capture の終わり） | 境界 | `moving_the_pointer_after_release_does_not_resize` |

**BR-22（動かした比を覚える。純関数は `dashboard.rs`、実走は `native/hayamimi/tests/br21.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-22-001 | 受入、純関数：比を読む関数に `{"cols":30,"rows":70}` | 読める値 | 値のまま | 30 と 70（specified：BR-22） | 陽性 | `br22_the_split_is_remembered_and_an_unreadable_one_falls_back_to_the_default` |
| T-BR-22-002 | 純関数：key の無い値、`not json`、`{"cols":"a"}`、`{"cols":120,"rows":70}`、`{"cols":30}`、`{"cols":null,"rows":70}`、`{"cols":true,"rows":70}` | 読めない 7 つ | 既定の 50 と 65 | どれも (50, 65)。`{"cols":120,"rows":70}` も rows を使わず (50, 65) になる（specified：設計 10.13.11「読めない時」と BR-22） | 陰性 | `an_unreadable_split_reads_as_the_default_ratio` |
| T-BR-22-003 | 純関数：`{"cols":0,"rows":100}` と `{"cols":100,"rows":0}` | 0 と 100 は範囲の内 | 読める | (0, 100) と (100, 0)（設計「0〜100 の外」の端の内側） | 境界 | `zero_and_one_hundred_are_readable_ratios` |
| T-BR-22-004 | 純関数：`{"cols":-1,"rows":70}` と `{"cols":50,"rows":100.5}` | 範囲の外側の端 | 既定 | どちらも (50, 65) | 境界 | `a_ratio_just_outside_zero_to_one_hundred_is_unreadable` |
| T-BR-22-005 | 実走：窓 1280 × 800。縦の線を 200 px drag、横の線を 100 px drag して離し、頁を読み直す | 覚えて読む | 動かした値 | 読み直した頁の `aria-valuenow` が 2 つとも読み直す前と同じ（± 1）で、左の列の幅が読み直す前と ± 2 px（specified：BR-22） | 陽性 | `a_moved_split_is_drawn_again_after_a_reload` |
| T-BR-22-006 | 実走：`localStorage` の値を `not json` に書いて開く | 読めない値 | 既定、書き換えない | 左右の列の幅が等しく（± 2 px）、上の列と清書の欄が 65 : 35（± 1 ポイント）、`localStorage` の値が `not json` のまま（specified：設計 10.13.11「画面に誤りの文は出さない」。頁の要素の数が正しい値の頁と同じ） | 陰性 | `an_unreadable_stored_split_draws_the_default_and_is_not_rewritten` |
| T-BR-22-007 | 実走：`Storage.prototype.getItem` が例外を投げるように差し替えた頁 | 使えない `localStorage` | 既定、止まらない | 既定の比で描かれ、`Runtime.exceptionThrown` が 0 件、`.row` が 4 つ（specified：BR-22 の陰性対照） | 陰性 | `a_throwing_local_storage_draws_the_default_and_the_page_keeps_running` |
| T-BR-22-008 | 実走：`Storage.prototype.setItem` が例外を投げる頁で縦の線を drag する | 書けない | その頁の memory の中でだけ効く | 2 回の drag とも左の列の幅が drag のとおり広がり、`Runtime.exceptionThrown` が 0 件、頁を読み直すと既定の比（50 と 65）で描かれる。`Runtime.consoleAPICalled` の `warning` のうち key `hayamimi.dashboard.split` を含む物が、2 回の drag を通して 1 件だけ。画面に誤りの文の要素が増えない（specified：設計 10.13.13 の 15。warn の数は、key の文字を含む warning の数で測る） | 境界 | `a_split_that_cannot_be_stored_still_resizes_and_raises_nothing` |
| T-BR-22-009 | 実走：`{"cols":5,"rows":70}` を書いて 1280 × 800 で開く | 読めたが最小に入らない | 描く時だけ寄せる | 左の列の幅が 239.5 以上 240.5 以下で、`aria-valuenow` が描いた幅の割合の丸め、`localStorage` の値が `{"cols":5,"rows":70}` のまま（specified：設計 10.13.11「描く時だけ最小へ寄せ、覚えた値は書き換えない」と 10.13.13 の 18） | 境界 | `a_remembered_ratio_below_the_minimum_is_drawn_clamped_and_kept_as_written` |
| T-BR-22-010 | 実走：縦の線を押して動かしている途中（mouse を離す前）の `localStorage` の値を読み、離した後にもう一度読む | 書く時 | 離した時 | 途中の値は動かす前と同じ文字列、離した後の値は動かした割合を含む（specified：設計 10.13.11「drag…離した時に覚える」） | 境界 | `the_split_is_stored_when_the_pointer_is_released` |
| T-BR-22-011 | 実走：縦の線に focus を置いて右の矢印を 1 回 | key で動かす | 1 回ごとに覚える | 押した後の `localStorage` の `cols` が押す前より 2 大きい（specified：設計 10.13.11「動かし終えた時」。key は 1 押しが 1 回の動き） | 境界 | `an_arrow_key_press_is_stored_after_each_move` |

**BR-23（左右の入れ替え。純関数は `dashboard.rs`、実走は `native/hayamimi/tests/br21.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-23-001 | 受入、実走：窓 1280 × 800。縦の線を drag して左の列を 30 にし、入れ替えの button を click。続けて Enter、Space | 1 回の操作 | 割合が 100 − c | click の後、`ja` のどの `.row` も縦の線の左、`en` のどの `.row` も右。左の列の割合が 70（± 1）、英語の側の幅が入れ替えの前と ± 2 px で等しい。`aria-pressed` が `true`。Enter で元の向きと割合 30（± 1）に戻り、Space で再び入れ替わる。頁を読み直しても入れ替えた向き（specified：BR-23） | 陽性 | `br23_one_swap_flips_the_sides_keeps_each_side_width_and_is_remembered` |
| T-BR-23-002 | 純関数：入れ替えの割合に 30、50、0、100 | 割合 | 100 − c | 70、50、100、0 | 境界 | `a_swap_turns_the_left_share_into_one_hundred_minus_it` |
| T-BR-23-003 | 純関数：0 から 100 までの整数 101 通りで、入れ替えを 2 回 | 全範囲 | 元に戻る | どの c でも 2 回で c に戻り、1 回の結果が 100 − c（property：式は 100 − c で、実装の外） | property | `swapping_twice_returns_every_share_to_itself` |
| T-BR-23-004 | 純関数：向きを読む関数に `"left":"ja"`、`"left":"en"`、`left` の無い値、`"left":"zh"`、`"left":1` | 向きの同値類 | `ja` と `en` だけ | `ja` は日本語の側が左、`en` と残り 3 つは英語の側が左。割合は同じ値のまま使う（specified：BR-23） | 境界 | `only_ja_and_en_are_read_as_a_remembered_side` |
| T-BR-23-005 | 実走：button に focus を置いて Enter の keyDown を 1 回、別に Space の keyDown と keyUp を 1 回 | key の起動 | 1 回の押しで 1 回 | どちらも `aria-pressed` が 1 回だけ反転する（Enter は keyDown で、Space は keyUp で起動。2 回反転しない）（specified：設計 10.13.11「button 要素にするので…1 回の操作で入れ替わる」） | 境界 | `enter_and_space_each_swap_exactly_once` |
| T-BR-23-006 | 実走：頁を開いた直後の `aria-pressed` | 既定の向き | `false` | `false`（specified：設計 10.13.11「既定の向きで false」） | 陽性 | `the_swap_button_starts_unpressed` |
| T-BR-23-007 | 実走：入れ替えを 2 回押す | 往復 | 元に戻る | 向き、割合、各 `.row` の側が押す前と同じ（specified：BR-23 の陽性対照） | 陽性 | `two_swaps_return_the_sides_the_share_and_every_row` |
| T-BR-23-008 | 実走：`left` を `"zh"` に書いた `localStorage` を読み直す | 外の値 | 既定、書き換えない | 英語の側が左で描かれ、`localStorage` の値が変わらない（specified：BR-23 の陰性対照） | 陰性 | `a_remembered_side_of_zh_draws_the_default_and_is_not_rewritten` |
| T-BR-23-009 | 実走：窓 600 × 900 の入れ替えの button | 狭い画面 | 見えない | 矩形の幅が 0 か `display: none`（specified：BR-23 の陰性対照） | 陰性 | `the_swap_button_is_hidden_on_a_narrow_page` |
| T-BR-23-010 | 実走：入れ替えた後に `localStorage` を読む | 覚える | `left` と割合 | 値に `"left":"ja"` と `"cols":70` を含む | 陽性 | `the_swapped_side_and_share_are_written` |
| T-BR-23-011 | 実走：`Storage.prototype.setItem` が例外を投げる頁で、入れ替えを 2 回押す | 向きを書けない | その頁の memory の中でだけ効く | 1 回目で `ja` の `.row` が縦の線の左、`aria-pressed` が `true`、2 回目で元に戻る。`Runtime.exceptionThrown` が 0 件。頁を読み直すと既定の向き（英語の側が左）。key `hayamimi.dashboard.split` を含む `warning` が 2 回の押しを通して 1 件だけ（specified：設計 10.13.13 の 15） | 境界 | `a_swap_that_cannot_be_stored_still_flips_for_this_page_and_warns_once` |

**BR-24（字体。純関数は `dashboard.rs`、実走は `native/hayamimi/tests/br24.rs`）**

H62 の決め：宣言した字体は `getComputedStyle` で、描いた字体は `CSS.getPlatformFontsForNode` で測る。計算された `font-family` は、`@font-face` の file が 404 でも変わらないので、配る 2 つの字体（`cinecaption`、`udev-gothic`）は描いた字体まで測る。端末に在る字体を名前で使う 3 つ（`yu-gothic`、`meiryo`、`yu-mincho`）は、測る機械が Linux で字体が無く、描かれる字体が機械で決まるので、描いた字体を記録に書くだけにして合否に置かない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-24-001 | 受入、実走：窓 1280 × 800、`hayamimi` に cinecaption と UDEV Gothic 35 の file の path を渡す。日本語の側の字体の `select` に focus を置き、矢印の key と Enter で `udev-gothic`。頁を読み直す | 側ごとの字体 | 先頭が `"UDEV Gothic 35"` | `ja` のどの `.row` の `.row-text` と `.tr-text` も `fontFamily` の先頭が `"UDEV Gothic 35"`、`en` の `.row` は `default` の並びのまま。読み直しても同じ。頁が出した要求の host がすべて頁の host と同じ（specified：BR-24） | 陽性 | `br24_the_chosen_font_reaches_the_computed_font_family_of_its_side_and_an_unknown_one_falls_back` |
| T-BR-24-002 | 純関数：字体を読む関数に 6 つの id（`default`、`cinecaption`、`udev-gothic`、`yu-gothic`、`meiryo`、`yu-mincho`） | 一覧の 6 つ | 先頭の字体の名 | 先頭が `Zen Kaku Gothic New`、`cinecaption`、`UDEV Gothic 35`、`Yu Gothic UI`、`Meiryo`、`Yu Mincho`。`yu-mincho` の並びは末尾が `serif`、ほかは `sans-serif`（specified：設計 10.13.11 の字体の表と 10.13.13 の 12「cinecaption の CSS の family 名は `cinecaption`」） | 陽性 | `each_listed_font_id_reads_as_its_fixed_family_list` |
| T-BR-24-003 | 純関数：`x; color: red`、`Comic Sans MS`、空文字、`UDEV-GOTHIC`、`udev-gothic `（末尾に空白）、数 1 | 一覧の外 | `default` | どれも `default` の並び（specified：BR-24。大文字小文字と空白を許さない） | 陰性 | `an_id_outside_the_list_reads_as_default` |
| T-BR-24-004 | 純関数：`;`、`}`、`{`、`url(`、改行、`\`、`/*`、`<`、`"` を含む 20 通りの文字列 | 差し込みの試み | 出力は固定の 6 つの並びのどれか | どの入力でも出力が 6 つの固定の並びのどれかと完全に等しく、入力の文字が出力に現れない（property：設計の「名前の文字列を利用者の入力から組まない」） | property | `no_input_text_ever_reaches_the_family_list` |
| T-BR-24-005 | 実走：`default` に戻す | 既定へ戻す | `--font-sans` の並び | `ja` の `.row-text` の `fontFamily` が、頁の中に作った `font-family: var(--font-sans)` の probe 要素の `fontFamily` と等しい（specified：BR-24 の陽性対照） | 陽性 | `going_back_to_default_restores_the_font_sans_list` |
| T-BR-24-006 | 実走：頁を開く間の `Network.requestWillBeSent` の host の集合。陽性対照として、別の port の localhost へ `fetch` を足した run の集合 | 外への要求 | 頁の host だけ | 通常の run の集合が頁の host 1 つだけ。対照の run の集合は 2 つで、述語が落ちる（specified：BR-24「外へ要求を出さず」。集合が 1 つの host で、対照が 2 つの時だけ通と読む） | 陽性、陰性 | `the_page_asks_no_host_other_than_its_own_for_fonts` |
| T-BR-24-007 | 実物：001 の後に `CSS.getPlatformFontsForNode` を `ja` の `.row-text` の node へ | 配る字体を描く | `familyName` が字体の file の family | `familyName` が、test が字体の file の `name` table（nameID 1）を別に読んだ family と等しく、`glyphCount` が 1 以上（H62：描いた字体まで測る。oracle は pseudo：file を読む別の小さい読み手） | 陽性 | `a_served_font_is_the_font_that_is_actually_painted` |
| T-BR-24-008 | 実物：字体の file の path を渡さずに起こし、`udev-gothic` を選ぶ | 宣言と描画の差 | 404 のとき描かない | `GET /fonts/udev-gothic` が 404、`fontFamily` の先頭は `"UDEV Gothic 35"` のまま、`getPlatformFontsForNode` の `familyName` が UDEV の family と等しくなく、`glyphCount` が 1 以上（頁が空白にならず、並びの次の字体で描く）（specified：設計 10.13.11「file が無い時は 404 を返し、並びの次の字体で描く」。H62 の宣言と描画の差を 007 の陰性対照にする） | 陰性 | `a_declared_font_whose_file_is_missing_is_not_the_one_painted` |
| T-BR-24-009 | 実物：配る file を渡した起動の `GET /fonts/udev-gothic` | 配る route | file と同じ bytes | 200 で、本文の sha256 が渡した file の sha256 と等しい（pseudo：別に打った `sha256sum`） | 陽性 | `the_font_route_serves_the_configured_file_byte_for_byte` |
| T-BR-24-010 | 実走：`GET /fonts/default`、`/fonts/yu-gothic`、`/fonts/x`、`/fonts/..%2f..%2fCargo.toml` | 一覧の外、file の無い id、遡る path | 404 | 4 つとも 404 で、本文に `[workspace]` が無い（specified：設計「決まった一覧の id」。陽性対照は 009 の 200） | 陰性 | `the_font_route_answers_404_for_unlisted_ids_and_climbing_paths` |
| T-BR-24-011 | 実走：`localStorage` の `ja.font` を `"x; color: red"` と `"Comic Sans MS"`、`ja.scale` を 120 で読み直す | 読めない値と読めた値の混在 | 項目ごと | `ja` の `.row-text` の `fontFamily` が `default` の並び、`fontSize` が `19.2px`、`color` が `red` でない、`localStorage` の値が書き換わらない（specified：BR-24 の陰性対照） | 陰性 | `an_unknown_font_is_replaced_but_the_readable_scale_is_kept_and_nothing_is_injected` |
| T-BR-24-012 | 実走：両方の側で `udev-gothic` を選び、`.lang-badge`、`.lat`、`h2`、`footer` の `fontFamily` を選ぶ前後で比べる | 字幕の外 | 変わらない | 4 つとも前後で同じ（specified：設計 10.13.11「見出し、言語の印、話者の印、遅れの数字、footer は今の字体のまま」） | 陰性 | `the_font_choice_does_not_touch_headings_badges_latencies_or_footer` |
| T-BR-24-013 | 実走：`ja` の側の字体を選び、清書の段落（`lang` が `ja`）の `.refine-text` と、日本語の速報の `.strip-text` | 4 つの字幕の文字 | 側の設定 | 4 つ（`.row-text`、`.tr-text`、`.refine-text`、`.strip-text`）の `fontFamily` の先頭がすべて選んだ字体（specified：設計 10.13.11） | 陽性 | `all_four_subtitle_texts_of_the_side_take_the_chosen_font` |
| T-BR-24-014 | 実物：`yu-gothic`、`meiryo`、`yu-mincho` を選び、それぞれの `getPlatformFontsForNode` の `familyName` を記録の file に書く | 端末の字体を名前で使う | 記録だけ | 3 つの `fontFamily` の先頭が宣言どおり。描いた `familyName` は記録だけで、合否に置かない（H62 の決め。落とさない） | 陽性 | `the_painted_family_of_each_device_font_is_recorded_and_not_judged` |
| T-BR-24-015 | 実走：`Storage.prototype.setItem` が例外を投げる頁で、`ja` の側の字体を `udev-gothic`、続けて `meiryo` にする | 字体を書けない | その頁の memory の中でだけ効く | 選ぶたびに `ja` の `.row-text` の `fontFamily` の先頭が選んだ字体（`"UDEV Gothic 35"`、次に `Meiryo`）になり、`en` は `default` のまま。`Runtime.exceptionThrown` が 0 件。頁を読み直すと `default`。key `hayamimi.dashboard.text` を含む `warning` が 2 回の選びを通して 1 件だけ（specified：設計 10.13.13 の 15） | 境界 | `a_font_that_cannot_be_stored_still_applies_for_this_page_and_warns_once` |

**BR-25（大きさ。純関数は `dashboard.rs`、実走は `native/hayamimi/tests/br24.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-25-001 | 受入、実走：英語の側の上げる button を Enter で 1 回、続けて 9 回、もう 1 回。下げる button を 12 回。既定へ戻す button | 倍率の歩み | 110 % から 200 %、80 % | 1 回で `en` の `.row-text` の `fontSize` が `17.6px`、`ja` は `16px`。9 回で `32px` で上げる button が `disabled`、もう 1 回押しても `32px`。下げる 12 回で `12.8px`。既定へ戻すで `16px`。Listening の欄の文が英字だけの時、`.strip-text` は `24px` × 倍率（specified：BR-25） | 陽性 | `br25_the_chosen_size_reaches_the_computed_font_size_within_its_bounds` |
| T-BR-25-002 | 純関数：倍率を読む関数に 80、90、100、150、200 | 範囲の内の 10 の倍数 | 値のまま | それぞれの値 | 陽性 | `multiples_of_ten_from_80_to_200_are_read_as_they_are` |
| T-BR-25-003 | 純関数：75、79、81、105、201、210、70、0、−10、`"a"`、`"100"`、null、true | 範囲の外、10 の倍数でない、数でない | 100 | どれも 100（specified：BR-25。210 と 70 は 10 の倍数で範囲の外、79 と 201 は範囲の端の外側） | 境界、陰性 | `a_scale_off_the_grid_or_off_the_range_or_not_a_number_reads_as_one_hundred` |
| T-BR-25-004 | 純関数：上げるを 190 と 200 に、下げるを 90 と 80 に | 端の歩み | 200 と 80 で止まる | 200、200、80、80。200 と 80 では「押せない」と返す | 境界 | `stepping_stops_at_the_two_ends` |
| T-BR-25-005 | 実走：80 から 200 まで 13 段のそれぞれで、`.row-text`、`.tr-text`、`.refine-text`、`.strip-text` の `fontSize` | 全段、4 つの文字 | 基準 × 倍率（± 0.01 px） | 基準は 16、16、16、24 px。80 % は 12.8、12.8、12.8、19.2。200 % は 32、32、32、48（property：式は基準 × 倍率で、実装の外） | property | `every_step_scales_the_four_texts_from_their_own_base_size` |
| T-BR-25-006 | 実走：005 の全段で、4 つの文字の `fontSize` の最小と最大 | 12.8 と 48 の線 | 12.8 以上 48 以下 | 最小が 12.8px、最大が 48px で、線を越える値が 0 件（specified：設計 10.13.11） | 境界 | `no_text_leaves_the_twelve_point_eight_to_forty_eight_pixel_band` |
| T-BR-25-007 | 純関数：Listening の欄の文の側を決める関数に、仮名と漢字 3 字とラテン文字 7 字の 10 字、4 字と 6 字の 10 字 | 3 割ちょうど | 3 割を超えれば日本語の側 | 前者は英語の側、後者は日本語の側（specified：設計 10.13.11 と 10.4 の 3。数えるのは `is_alphabetic` の文字） | 境界 | `a_strip_text_with_exactly_three_tenths_kana_and_kanji_uses_the_english_side` |
| T-BR-25-013 | 純関数：同じ関数に、空の文字列、`。、！`（字が 0）、頁の言語の「待ち」の文言を、組 `en,ja` と組 `ja,en` で渡す | 字が 1 つも無い | `<a>` の側 | 組 `en,ja` では 3 つとも英語の側、組 `ja,en` では 3 つとも日本語の側（3 割の線を使わない）（specified：設計 10.13.13 の 26。陽性対照は 007 の字の在る文） | 境界 | `a_strip_text_with_no_letters_uses_the_side_of_the_first_language_of_the_pair` |
| T-BR-25-008 | 実走：上げる button を押した後の `aria-live` の要素の text | 読み上げ | 今の値 | text が `110` を含む（specified：設計 10.13.11「倍率の今の値を aria-live の文で読み上げる」） | 陽性 | `the_live_region_announces_the_current_scale` |
| T-BR-25-009 | 実走：110 % にして頁を読み直す | 覚える | 同じ倍率 | `.row-text` の `fontSize` が `17.6px`（specified：BR-25 の陽性対照） | 陽性 | `a_chosen_scale_is_drawn_again_after_a_reload` |
| T-BR-25-010 | 実走：`en.scale` を 250 に書いて読み直す（`en.font` は `udev-gothic`） | 範囲の外 | 100 | `en` の `.row-text` が `16px`、`fontFamily` の先頭が `"UDEV Gothic 35"` のまま（specified：BR-25 の陰性対照） | 陰性 | `a_remembered_scale_of_250_draws_one_hundred_and_keeps_the_font` |
| T-BR-25-011 | 実走：英語の側の倍率を 150 % にする | 側の独立 | `ja` は動かない | `ja` の `.row-text` が `16px` のまま、`.strip-text` は日本語の文なら `24px` | 陰性 | `the_english_scale_does_not_resize_the_japanese_side` |
| T-BR-25-012 | 実走：`Storage.prototype.setItem` が例外を投げる頁で、英語の側の上げる button を 3 回 | 倍率を書けない | その頁の memory の中でだけ効く | `en` の `.row-text` の `fontSize` が `20.8px`（130 %）になり、`ja` は `16px`。`Runtime.exceptionThrown` が 0 件。頁を読み直すと `16px`。key `hayamimi.dashboard.text` を含む `warning` が 3 回の押しを通して 1 件だけ（specified：設計 10.13.13 の 15） | 境界 | `a_scale_that_cannot_be_stored_still_applies_for_this_page_and_warns_once` |

**言語の組の行（BR-26 から BR-31）の共通**：言語の表の期待値は、test が表の file（`native/hayamimi-relay/data/llm_langs.toml`）を別に読んで組む。`--models` の fixture は、`tiers.rs` の表で言語を読む認識器の file だけを空の file で置いた directory で、置いた言語の一覧は test が決める。`POST /config` の呼び手は loopback の client である。`langs` の event は `/events` の SSE で読み、形は `{"type":"langs","pair":["<a>","<b>"]}`。

**BR-26（選べる言語と組。`native/hayamimi/tests/br26.rs`、中継は `native/hayamimi-relay/tests/br26.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-26-001 | 受入：`--models` の fixture に `ja`、`en`、`zh` の認識器の file だけを置いて起こし、`GET /langs`。中継は偽の訳の server の `/props` の `model_path` を表の 2 つの model の名にして起こし、`GET /langs` | 認識の側と訳の側 | 手元の model | `hayamimi` の `/langs` が `ja`、`en`、`zh` を返し、`ko` を返さない。中継の `/langs` が表の言語から作った組だけを返す（oracle は表を別に読んだ組。specified：BR-26） | 陽性 | `br26_the_choosable_languages_come_from_the_models_on_this_machine` |
| T-BR-26-002 | 中継：表に CAT-Translate の名を置いた偽の server | 表に在る model | 組が返る | `en,ja` の組が返る（specified：BR-26 の陽性対照） | 陽性 | `a_model_in_the_table_gives_its_pairs` |
| T-BR-26-003 | 中継：`--llm-other` の偽の server の `model_path` が表に無い名（`unknown-model.gguf`）。`/langs` を 3 回引く | `--llm-other` の表に無い model | その server の組は 0 | その server の組が 0、warn が起動で 1 行だけ（3 回引いても増えない）（specified：BR-26 の陰性対照と設計 10.13.12 の 2「`--llm-other` の server は、表に名が在る時だけ表の言語を持つ」） | 陰性 | `a_llm_other_model_missing_from_the_table_has_no_pairs_and_warns_once` |
| T-BR-26-004 | 中継：`--llm` の偽の server の `model_path` が表に無い名（CAT-Translate の file 名を含む）。`/langs` を 3 回引く | `--llm` の表に無い model | `en,ja` の 1 つだけ | 組の一覧がちょうど `en,ja` の 1 つで、warn が起動で 1 行だけ（specified：設計 10.13.12 の 2「`--llm` の server は…表に名が無ければ `en,ja` の組だけ」と BR-26 の陰性対照） | 境界 | `a_llm_model_missing_from_the_table_still_gives_en_ja_and_warns_once` |
| T-BR-26-005 | `--pair ko,ja` で起こす（fixture は `ja`、`en`、`zh`） | 認識の側の一覧に無い言語 | 起動が止まる | exit code が 0 でなく、理由に `ko` が出て、`--serve` の port が一度も開かない（specified：BR-26 の陰性対照） | 陰性 | `a_pair_with_a_language_that_cannot_be_read_stops_the_start` |
| T-BR-26-006 | `--pair` が `ja,ja`、`ja`、`en,ja,zh`、`en,`、`,ja`、`"en, ja"`、`eng,jpn`、`e1,ja` | 同じ言語、要素が 2 つでない、空の要素、空白を含む、code の形でない | ちょうど 2 つの時だけ受ける | 8 つとも起動が止まり、理由が出て、exit code が 0 でない。`"en, ja"` は空白を削らずに比べるので止まる（specified：設計 10.13.12 と 10.13.13 の 25。陽性対照は 011 の `zh,ja`） | 境界 | `a_malformed_or_repeated_pair_stops_the_start` |
| T-BR-26-007 | `--models` の fixture に `ja` と `en` の file だけを置く（`zh` の file が無い） | tier は在るが file が無い | 選べない | `/langs` が `zh` を返さない（specified：設計 10.13.12「model の file が置き場に在る時に選べる」） | 境界 | `a_language_whose_model_file_is_absent_is_not_offered` |
| T-BR-26-008 | `--models` が空の directory | 何も無い | 空の一覧 | `/langs` が空の一覧を返し、起動が panic しない | 境界 | `an_empty_models_directory_offers_no_language` |
| T-BR-26-009 | 中継：`--llm` の server が `en` と `ja`、`--llm-other` の server が `zh` と `ja` を持つ | 2 つの server に言語が分かれる | 組は 1 つの server が両方を持つ時 | 組の一覧に `en,ja` と `ja,zh` が在り、`en,zh` が無い（specified：設計 10.13.12「訳の server のどれかが a と b の両方を持つ時」。組を順序なしの 2 つの言語として数える。順序の扱いは残る穴） | 境界 | `a_pair_needs_one_translation_server_that_has_both_languages` |
| T-BR-26-010 | `--pair` を渡さずに起こして `/events` を開く | 既定の組 | `en,ja` | 最初に流れる `langs` が `["en","ja"]`（specified：設計 10.13.12「`--pair` が無ければ `en,ja`」） | 陽性 | `without_a_pair_flag_the_pair_is_en_ja` |
| T-BR-26-011 | `--pair zh,ja`（fixture に `zh` と `ja`）で起こして `/events` を開く | 並びの順 | `<a>` が先 | `langs` が `["zh","ja"]`（`["ja","zh"]` でない）（specified：設計 10.13.12「並びの順は頁の既定の左右で、`<a>` が左」） | 境界 | `the_given_order_of_the_pair_is_kept_in_the_langs_event` |

**BR-27（組が判定と落ち先と訳の向きを決める。`native/hayamimi-asr/tests/br27.rs`、中継は `native/hayamimi-relay/tests/br27.rs`）**

材料：判定は T-IP-03 の偽の形（`Named`、`Models`、`Script`、`FixedLid`）で、組を `Policy::pair` へ渡す引数にする。SenseVoice の偽は印を返す。`zh,ja` の文字の段は仮名が 1 字でも在れば `ja`、無ければ `zh`（設計 10.13.13 の 9）で、最後の落ち先は `lid.md:17` の clean の 1.0 秒の行（ja 33 %、en 93 %、zh 8 %、ko 33 %、yue 0 %）の正解率の低い方、同じ値なら `<a>`（設計 10.13.12）である。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-27-001 | 受入：組 `zh,ja`。偽の判定器の答えが `zh`、`ja`、`en`、`ko`、空、`Err`。偽の SenseVoice の印は常に `zh` | 組の中の答え、組の外の答え、答えなし | 選ばれる言語は組の中 | `zh` の答えは `zh`（落ちない）、`ja` の答えは `ja`（落ちない）。`en`、`ko`、空、`Err` の 4 つは `zh` で `step` が `sv_tag`。6 つとも選ばれる言語が `zh` か `ja`（specified：BR-27。SenseVoice の印が組の中なら、その印に従う） | 陽性 | `br27_the_pair_on_the_command_line_decides_identification_fallback_and_direction` |
| T-BR-27-002 | 組 `zh,ja`。判定器 `en`、SenseVoice の印 `ja` | 印が組のもう一方 | 印に従う | `ja`、`step` が `sv_tag` | 陽性 | `a_sensevoice_tag_for_the_other_language_of_the_pair_is_followed` |
| T-BR-27-003 | 組 `en,ja`（既定）で `-p hayamimi-asr --test ip03` を打つ | 既定の組 | 変わらない | T-IP-03 の全行が変更前と同じに通る（specified：BR-27 の陽性対照。受入 test の名前は `ip03_an_unsure_identifier_falls_to_ja_or_en`） | 陽性 | `ip03_an_unsure_identifier_falls_to_ja_or_en` |
| T-BR-27-004 | 組 `ko,en`。判定器 `zh`、SenseVoice の印 `zh`、文 `안녕하세요 여러분`（Hangul だけ） | 字の組が違う 2 つ、文が一方の字 | 3 割を超えれば `ko` | `ko`、`step` が `script`（specified：設計 10.13.12 の 3。T-IP-03-006 の読み替え） | 陽性 | `a_hangul_text_is_ko_in_a_ko_en_pair` |
| T-BR-27-005 | 組 `ko,en`。同じ設定で文 `hello there` | ラテン文字だけ | `en` | `en`、`step` が `script` | 陽性 | `a_latin_text_is_en_in_a_ko_en_pair` |
| T-BR-27-006 | 組 `ko,en`。文が Hangul 3 字とラテン文字 7 字、続けて Hangul 4 字とラテン文字 6 字 | 3 割ちょうどと 1 字多い | 3 割を超えれば `ko` | 前者は `en`、後者は `ko`（specified：今の 3 割の線。T-IP-03-006 と 029 の読み替え。境界の両側） | 境界 | `exactly_three_tenths_hangul_is_en_and_one_more_letter_is_ko` |
| T-BR-27-007 | 組 `ko,en`。SenseVoice が `Unavailable`、(b) 復号の失敗、(c) 空の文 | 判定の段が無い、失敗、文が無い | 落ち先は正解率の低い方 | (a) `ko`、`step` が `default`。(b) `ko`、`step` が `sv_failed`。(c) `ko`、`step` が `default`（specified：設計 10.13.12 の 4 と 5。`lid.md` の表 1 は `ko` が `en` より全 condition で低い：clean 1.0 秒で 33 % と 93 %） | 境界 | `a_ko_en_pair_falls_to_ko_where_whisper_tiny_is_less_often_right` |
| T-BR-27-008 | 組 `es,fr`（どちらも SenseVoice の 5 つの外）。判定器 `de`、SenseVoice の偽は呼ばれたら落とす | 組の言語が 5 つの外 | 2 を飛ばす | SenseVoice が 0 回呼ばれ、`es`、`step` が `default`（specified：設計 10.13.12「5 つの外の時は 2 を飛ばして 3 へ進む」。字の組が同じ（ラテン文字）なので 3 も飛ばす。表に無い言語は `<a>`） | 境界 | `a_pair_outside_the_sensevoice_languages_skips_the_sensevoice_step` |
| T-BR-27-009 | 組 `fr,es`。008 と同じ入力 | `<a>` の側 | `<a>` | `fr`（`es,fr` の 008 が `es` なので、`<a>` が落ち先を決めることを見る） | 境界 | `the_first_language_of_the_pair_is_the_last_fallback_when_neither_is_in_the_table` |
| T-BR-27-010 | 中継：組 `zh,ja` の `langs` を渡し、`zh` の `final` を流す | 組の一方から他方 | 反対の向き | `lang: ja`、`src_lang: zh`、`status: ok` の translation が 1 件（specified：BR-27） | 陽性 | `a_zh_final_is_translated_into_ja_when_the_pair_is_zh_ja` |
| T-BR-27-011 | 同じ中継に `ja` の `final` | 逆の向き | 反対の向き | `lang: zh`、`src_lang: ja` の translation が 1 件 | 陽性 | `a_ja_final_is_translated_into_zh_when_the_pair_is_zh_ja` |
| T-BR-27-012 | 同じ中継に `en` の `final` | 組の外 | 訳さない | translation が 0 件、`translate.line.skipped` が 1 件で `reason` が `language`（specified：BR-27 の陰性対照） | 陰性 | `a_final_outside_the_pair_is_skipped_for_language` |
| T-BR-27-013 | 中継：`langs` を受ける前に `zh` の `final`、続けて組 `zh,ja` の `langs`、続けて `zh` の `final` | 組を event から読む | event で変わる | 前の `final` は skipped（既定の `en,ja` の外）、後の `final` は translation が 1 件（specified：設計 10.13.12「中継と頁は、この event から組を読み、自分では組を持たない」） | 境界 | `the_relay_reads_the_pair_from_the_event_and_not_from_itself` |
| T-BR-27-014 | 中継：どの server も持たない組 `ko,fr` の `langs`、続けて `ko` の `final` | 訳せない組 | fallback の event と skipped | `{"type":"fallback","stage":"translate","reason":"unsupported_pair"}` が 1 件、その `final` は `translate.line.skipped`（`reason: language`）（specified：設計 10.13.12「中継が組を訳せない時」） | 陰性 | `a_pair_no_server_can_translate_is_reported_as_unsupported` |
| T-BR-27-015 | 中継：組 `zh,ja` で `zh` の `final` を `FakeLlm` に通す | `ja` と `en` の外の言語から | T と V だけ | E の段の依頼が 0 件、T の依頼が 1 件以上（specified：設計 10.13.12「`ja` と `en` の外は 3.4 のとおり T と V だけ」） | 境界 | `a_translation_from_a_language_without_tables_goes_through_the_first_stage_only` |
| T-BR-27-016 | 中継：組 `zh,ja`。`zh` の速報の訳（原文 `你好`）を追い、同じ `id` の `final` の text が `你好。` | 速報と確定文の比べ（H65） | `ja` と同じ働き | その `final` の T の依頼が 0 件で、速報の訳の文が `final` の訳として出る（`ja` の `。` の扱いと同じ。derived：設計 10.13.10 の H65 が問う「漢字だけの `zh` で同じに働くか」の答えを、`ja` と同じ働きと決める。実装が `Lang::Ja` の比べを直に読んでいるなら、この行は落ちる） | 陽性 | `a_zh_final_with_a_closing_mark_reuses_the_partial_translation_like_ja` |
| T-BR-27-017 | 中継：組 `zh,ja`。速報の原文が `你 好`、`final` の text が `你好` | 空白の違い | `ja` と同じく空白を比べない | 使い回しが起きる（T の依頼が 0 件）（derived：H65。`ja` は空白を外して比べる：`same_text` の `japanese`） | 境界 | `a_zh_final_that_differs_only_in_spaces_reuses_the_partial_translation` |
| T-BR-27-018 | 中継：組 `en,es`。速報の原文が `Hola mundo`、`final` の text が (a) `Hola mundo`、(b) `Hola mundo.`、(c) `hola mundo` | ラテン文字の言語 | `en` と同じく完全一致だけ | (a) は使い回し、(b) と (c) は使い回さない（T の依頼が 1 件）（derived：H65。`en` の比べは `japanese` が偽で、文字どおりの一致だけ） | 境界、陰性 | `a_latin_script_pair_reuses_a_partial_translation_only_on_an_exact_match` |
| T-BR-27-019 | 中継：組 `ko,en`。速報の原文が `안녕하세요 여러분`、`final` の text が同じ字と空白 | 空白を持つ非ラテン文字の言語 | `en` と同じく完全一致 | 使い回しが起きる。空白を 1 つ減らした `final` では起きない（derived：H65。`ko` は語の間に空白を持つので、空白を外して比べる `ja` の規則ではなく、`en` の規則） | 境界 | `a_ko_final_is_compared_with_its_spaces` |
| T-BR-27-020 | 中継：速報の訳を追っている最中に、組 `zh,ja` の `langs` を受け、同じ `id` の `final` を流す | 組が変わった時の控え | 控えを捨てる | その `final` の T の依頼が 1 件（使い回さない）。前の組で頼んだ訳の答えは、その `id` の行に付く（specified：設計 10.13.12「組を変えた時に…速報の使い回しの控えを捨てる。前の組で頼んだ訳の答えは今のとおり id の行に付ける」） | 陰性 | `a_pair_change_drops_the_tracked_partial_so_the_final_is_not_reused` |
| T-BR-27-021 | 組 `zh,ja`。判定器 `ko`、SenseVoice の印 `ko`、文 `こんにちは世界` | 漢字を使う 2 つの言語、文に仮名が在る | 仮名が 1 字でも在れば `ja` | `ja`、`step` が `script`（specified：設計 10.13.13 の 9） | 陽性 | `a_text_with_kana_is_ja_in_a_zh_ja_pair` |
| T-BR-27-022 | 組 `zh,ja`。021 と同じ設定で文 `你好世界` | 仮名が 0 字 | 無ければ `zh` | `zh`、`step` が `script`（同上。022 と 023 で仮名の 0 字と 1 字の両側を見る） | 境界 | `a_text_without_kana_is_zh_in_a_zh_ja_pair` |
| T-BR-27-023 | 組 `zh,ja`。021 と同じ設定で文 `你好の世界` | 仮名がちょうど 1 字 | 1 字でも在れば `ja` | `ja`、`step` が `script` | 境界 | `a_single_kana_makes_the_text_ja_in_a_zh_ja_pair` |
| T-BR-27-024 | 組 `zh,ja`。判定器 `ko`。SenseVoice が (a) `Unavailable`、(b) 復号の失敗、(c) 空の文 | 判定の段が無い、失敗、文が無い | 落ち先は正解率の低い方 | (a) `zh`、`step` が `default`。(b) `zh`、`step` が `sv_failed`。(c) `zh`、`step` が `default`（specified：設計 10.13.12 の 4 と 5 と `lid.md:17` の zh 8 %、ja 33 %） | 境界 | `a_zh_ja_pair_falls_to_zh_where_whisper_tiny_is_less_often_right` |
| T-BR-27-025 | 組 `ja,zh`（`<a>` が `ja`）。024 の (a) と同じ入力 | 順序を替えた組 | 低い方は順序に依らない | `zh`、`step` が `default`（`<a>` の `ja` でなく、正解率の低い `zh`） | 境界 | `the_fallback_follows_the_lower_accuracy_and_not_the_order_of_the_pair` |
| T-BR-27-026 | 組 `ko,ja` と `ja,ko`。024 の (a) と同じ入力 | 正解率が同じ（clean 1.0 秒で 33 % と 33 %） | 同じ値なら `<a>` | `ko,ja` は `ko`、`ja,ko` は `ja`、どちらも `step` が `default`（specified：設計 10.13.12「同じ値なら `<a>`」） | 境界 | `equal_accuracy_falls_to_the_first_language_of_the_pair` |
| T-BR-27-027 | 組 `en,es`。判定器 `de`、SenseVoice の偽は印 `en` を返し、呼ばれた回数を数える。文 `hello there` | 組の片方だけが 5 つの外 | 2 を組ごと飛ばす | SenseVoice の呼びが 0 回（印 `en` に従わない）。字の組が同じ（ラテン文字）なので 3 も飛ばし、`es` は表に無いので `<a>` の `en`、`step` が `default`（specified：設計 10.13.13 の 17 と 10.13.12 の 3、4、5。BR-27 の陰性対照 `en,es`） | 陰性 | `a_pair_with_one_language_outside_the_five_skips_the_sensevoice_step_whole` |
| T-BR-27-028 | 組 `es,en`。027 と同じ入力 | 順序を替えた組 | `<a>` | SenseVoice の呼びが 0 回、`es`、`step` が `default`（`<a>` が落ち先を決める） | 境界 | `the_first_language_decides_the_fallback_of_a_pair_with_one_language_outside_the_five` |

**BR-28（途中で組を変える。`native/hayamimi/tests/br28.rs`）**

材料：音は `--input ws` に test 自身の client で流し、`POST /config` を送る時点を test が決める。英語の clip は `eval_real` の英語、中国語の clip は `eval_real_zhko` の中国語を使う。`asr.model.loaded` は log の行の時刻と、`langs` を受けた時刻（同じ機械の時計）を比べる。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-28-001 | 受入、実走：`en,ja` で英語の 2 発話を流し、`POST /config` で `{"pair":["zh","ja"]}`。続けて中国語の 1 発話を流す | 変える前の行と変えた後の区間 | 新しい組は次の区間から | `langs` が 1 件流れる。変えた後の区間の `final` の `lang` が `zh` か `ja`。変える前の 2 件の `final` と訳は流し直されず、`id` も変わらない（specified：BR-28） | 陽性 | `br28_a_pair_changed_mid_run_reads_the_next_segment_and_leaves_earlier_lines` |
| T-BR-28-002 | 001 の run の log と `langs` を受けた時刻 | 認識器を先に読む | 読み終えてから流す | `asr.model.loaded`（`zh` の認識器）の行の時刻が、`langs` を受けた時刻以前（specified：BR-28 の陽性対照。設計 10.13.12 の 4） | 陽性 | `the_new_recognizer_is_loaded_before_the_langs_event_is_sent` |
| T-BR-28-003 | 英語の clip の途中で `POST /config` を送り、clip の残りと無音を流す | 変えた時に開いていた区間 | 前の組で読む | その区間の `final` の `lang` が `en`（前の組の言語）で、後の区間（中国語）の `lang` が `zh` か `ja`（specified：設計 10.13.12「変えた時に開いていた区間は、前の組で読み」） | 境界 | `a_segment_open_at_the_change_is_read_in_the_old_pair` |
| T-BR-28-004 | 001 の run で、変えた後の `/events` に出た `id` | 流し直し | 重ならない | 変える前の 2 つの `id` が `final` として 1 回ずつだけ（変えた後に再び出ない）、その 2 つへの `translation` が変えた後に新しく出ない（specified：BR-28「流し直されず」） | 陰性 | `earlier_finals_and_their_translations_are_not_sent_again_after_the_change` |
| T-BR-28-005 | 認識器の file の無い組（`ko,ja`、`ko` の file が無い fixture）へ `POST /config` | 読めない組 | 変えない | 応答が 2xx でなく、`langs.change.failed` が error で 1 件、`langs` が流れず、続く英語の clip が `en` で読まれる（specified：BR-28 の陰性対照） | 陰性 | `a_pair_that_cannot_be_loaded_is_refused_and_the_old_pair_keeps_working` |
| T-BR-28-006 | 001 の変更の後で、新しく `/events` を開く | 保った event | 新しい組を流し直す | その接続が最初のほうで受ける `langs` が `["zh","ja"]`（specified：設計 10.13.12「新しい接続へ session_start と fallback と同じく流し直し」） | 陽性 | `a_new_connection_after_the_change_is_told_the_new_pair` |
| T-BR-28-007 | `langs` の 1 件の JSON | 形 | 2 つの文字列 | `pair` が長さ 2 の配列で、どちらも 2 字の小文字、順が `POST` の順（specified：設計 10.13.12） | 陽性 | `the_langs_event_carries_exactly_two_codes_in_the_given_order` |
| T-BR-28-008 | 実物：001 の run の `final` の項目の集合と、`docs/results/in-person/ip04_pair.events.jsonl` の `final` の項目の集合 | `final` の項目は変えない | 同じ集合 | 2 つの集合が等しい（`audio_s`、`decode_ms`、`end_wall_ms`、`id`、`lang`、`latency_ms`、`lid_ms`、`source`、`speaker`、`switched`、`t0_s`、`t1_s`、`text`、`tier`、`type`）（derived：2026-10-23 に `jq` で数えた記録の項目。設計 10.13.12「`final` と `refine` の項目は変えない」） | 陰性 | `the_final_event_keeps_its_fields_when_langs_is_added` |

**BR-29（組を変える口は loopback からだけ。`native/hayamimi/tests/br29.rs`）**

材料：`--serve-host 0.0.0.0` で起こし、test が自分の loopback でない address を、送信せずに経路から読む（`UdpSocket` を `connect` するだけで送らない）。その address が取れない機械では、行は通でなく「loopback でない address が無い」で落ちる。address の判定の純関数は「相手の address」を受けて受けるか返す。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-29-001 | 受入：loopback の address から `POST /config` の `{"pair":["zh","ja"]}`、続けて loopback でない自分の address から同じ要求 | loopback と LAN | 403 | 前者が 2xx で `langs` が流れ、後者が 403 で組が変わらない（`GET /config` か `/events` の `langs` で見る）（specified：BR-29） | 陽性 | `br29_only_a_loopback_page_may_change_the_pair` |
| T-BR-29-002 | loopback でない address から `{"punctuate":false}` | `pair` 以外の項目 | 今のとおり | 2xx（specified：BR-29 の陽性対照） | 陽性 | `a_lan_client_can_still_set_the_other_fields` |
| T-BR-29-003 | loopback から、本文が JSON でない `not json`、`{"pair":"ja,en"}`、`{"pair":null}`、`{"pair":{}}`（配列でない）、`{"pair":[]}`、`{"pair":["ja"]}`、`{"pair":["ja","en","zh"]}`（要素の数が 2 でない）、`{"pair":[1,2]}`、`{"pair":["ja",1]}`（文字列でない）、`{"pair":["JA","en"]}`、`{"pair":["j","en"]}`、`{"pair":["japanese","en"]}`、`{"pair":["j1","en"]}`（code の形でない）、`{"pair":["ja","ja"]}`、`{"pair":["xx","ja"]}`（認識の側の一覧に無い） | 不正な組 7 種 | 400 | 15 個とも 400 で組が変わらず、`langs` が流れない（specified：BR-29 の陰性対照と設計 10.13.13 の 24 の 7 つの条件。陽性対照は 001 の受ける組） | 陰性 | `an_invalid_pair_is_a_bad_request_and_changes_nothing` |
| T-BR-29-004 | loopback でない address から `{"pair":["ja","ja"]}`、本文が JSON でない `not json`、正しい組 `{"pair":["zh","ja"]}` | 認可と検証の順 | 認可が先 | 3 つとも 403 で、本文の中身によらず、組が変わらない（specified：設計 10.13.13 の 24「認可を先に、検証を後に見る。本文を読まない」） | 境界 | `a_lan_client_is_refused_before_its_body_is_judged` |
| T-BR-29-005 | loopback から `["zh","ja"]` の後に `["ja","zh"]` | 順序の違う同じ 2 言語 | どちらも受ける | どちらも 2xx で、`langs` が 1 件ずつ流れ、`pair` がそれぞれ送った順。順序だけを変えた 2 回目の後も、`hayamimi` の log に組の変更を知らせる行が増えない（specified：設計 10.13.13 の 7。頁の区切りと `left` は BR-31 の 006 が見る） | 境界 | `the_same_two_languages_in_the_other_order_are_a_different_valid_pair` |
| T-BR-29-006 | 純関数：相手の address に 126.255.255.255、127.0.0.0、127.255.255.255、128.0.0.0、`::1`、`::`、`::ffff:127.0.0.1`、192.168.1.5、10.0.0.1、0.0.0.0 | loopback の両端と外 | loopback だけ | 127.0.0.0、127.255.255.255、`::1`、`::ffff:127.0.0.1` を受け、残りの 6 つを断る（specified：設計 10.13.12「相手の address が loopback の時だけ」と 10.13.13 の 6。`::ffff:127.0.0.1` は loopback として受ける） | 境界 | `only_loopback_peers_may_change_the_pair` |

**BR-30（頁の左右と設定が組に従う。純関数は `dashboard.rs`、実走は `native/hayamimi/tests/br30.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-30-001 | 受入、実走：`--pair zh,ja` で起こし、窓 1280 × 800 の頁に `zh` の行と `ja` の行を流す。`localStorage` の `left` を `"en"` にして別の頁を開く。`zh` の字体を選ぶ | 組の中の言語、覚えた向きが組に無い、設定の鍵 | `<a>` が左 | `zh` の `.row` が縦の線の左、`ja` の `.row` が右。`#pair-a` の value が `zh`、`#pair-b` の value が `ja`。`left` が `"en"` の頁でも `zh` が左。`zh` を選ぶと `zh` の `.row` の `fontFamily` に出る。`{"ja":…}` の設定は `ja` の `.row` に効く（specified：BR-30。組を選ぶ口は `.pair-picker` の `#pair-a` と `#pair-b` で、その value が `zh` と `ja`：設計 10.13.13 の 12） | 陽性 | `br30_the_page_sides_and_text_settings_follow_the_pair` |
| T-BR-30-002 | 純関数：行の側を決める関数に、組 `[zh, ja]` で `zh`、`ja`、`en`、`ko`、空 | 組の中、組の外 | 組の外は `<a>` の側 | `zh` は `<a>` の側、`ja` は `<b>` の側、`en`、`ko`、空は `<a>` の側（specified：設計 10.13.12「どちらでもなければ組の `<a>` の側」） | 境界 | `a_line_outside_the_pair_goes_to_the_side_of_the_first_language` |
| T-BR-30-003 | 純関数：向きを読む関数に、組 `[zh, ja]` で `"left":"ja"`、`"left":"zh"`、`"left":"en"`、`left` 無し | 覚えた向きと組 | 組に在る言語だけ | `ja` は `ja` が左、`zh` は `zh` が左、`en` と無しは `zh` が左（specified：設計 10.13.12「覚えた `left` が今の組に在れば…無ければ組の並びの既定」） | 境界 | `a_remembered_left_that_is_not_in_the_pair_falls_to_the_first_language` |
| T-BR-30-004 | 純関数：設定を読む関数に、組 `[zh, ja]` で `{"ja":{"font":"udev-gothic","scale":120},"ko":{"font":"meiryo","scale":200}}` | 鍵と組 | 組の鍵だけ | `ja` は (udev-gothic, 120)、`zh` は (default, 100)。`ko` はどこにも出ない（specified：BR-30 の陰性対照） | 陰性 | `only_the_keys_of_the_current_pair_are_read` |
| T-BR-30-005 | 純関数：同じ保存の値を、組 `[en, ja]`、`[zh, ja]`、`[en, ja]` の順に読む | 組を替えて戻す | 戻る | 3 回目の `en` の設定が 1 回目と等しい。保存の値は 3 回とも読むだけで変わらない（specified：設計 10.13.12「組を替えて戻した時は、前に選んだ字体と大きさが戻る」） | property | `switching_the_pair_away_and_back_restores_the_earlier_text_settings` |
| T-BR-30-006 | 実走：`en,ja` で起こした頁に BR-19 から BR-25 の述語を打つ | 既定の組 | そのまま通る | BR-19 の 001、BR-21 の 001、BR-23 の 001、BR-24 の 001、BR-25 の 001 の述語が通る（specified：BR-30 の陽性対照） | 陽性 | `the_default_pair_page_still_passes_the_side_and_text_predicates` |
| T-BR-30-007 | 実走：`localStorage` に `{"ko":{"font":"udev-gothic","scale":200}}` を置いて `zh,ja` の頁を開く | 組に無い鍵 | どこにも効かない | どの `.row-text` も `default` の並びと `16px` | 陰性 | `a_setting_for_a_language_outside_the_pair_changes_no_row` |
| T-BR-30-008 | 実走：loopback の頁で `#pair-a` の value を `zh`、`#pair-b` を `ja` にして `#pair-apply` を click | 組を選ぶ口 | `POST /config` の `pair` | `/events` に `langs` が `["zh","ja"]` で 1 件流れる。口の選択肢は、頁が読んだ 2 つの `/langs` の両方に在る言語だけ（specified：設計 10.13.12「頁は 2 つの `/langs` を読み、両方に在る組だけ」と 10.13.13 の 12） | 陽性 | `the_pair_picker_sends_the_chosen_pair_to_the_config_endpoint` |

**BR-31（組を変えた所の区切りの行。`native/hayamimi/tests/br30.rs`）**

材料：区切りの行は `div.pair-divider` で、`data-pair` に新しい組を `<a>,<b>` の形で持つ。組を選ぶ口は `.pair-picker` の欄で、`#pair-a`、`#pair-b`、`#pair-apply` を持つ（設計 10.13.13 の 12）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-31-001 | 受入、実走：`en,ja` の頁に `en` と `ja` の 2 行を流した後、loopback の頁から `zh,ja` へ変え、続けて `zh` の行を流す | 変えた所 | 区切り 1 つ | `.pair-divider` が 1 つ入り、`.main` の左から右にまたがる（± 2 px）。`data-pair` が `zh,ja`。前の `en` の行は縦の線の左、`ja` の行は右のまま。区切りより後の `zh` の行は左。区切りの text が `zh` と `ja` の code を含む（specified：BR-31 と設計 10.13.13 の 12） | 陽性 | `br31_a_pair_change_draws_a_divider_and_keeps_earlier_lines_where_they_arrived` |
| T-BR-31-002 | 実走：組を変えない頁に 2 行を流す | 変えない | 区切り 0 | 区切りが 0 個（specified：BR-31 の陽性対照） | 陽性 | `a_page_whose_pair_never_changes_has_no_divider` |
| T-BR-31-003 | 実走：001 の run の前の行の `.tr-text` と `.row-text` の text を、変える前後で比べる | もう出ている行を訳し直さない | 変わらない | 前の行の text が前後で同じ（specified：設計 10.13.12「消さず、訳し直さず」） | 陰性 | `earlier_rows_are_neither_removed_nor_translated_again` |
| T-BR-31-004 | 実走：loopback でない address で開いた頁（`--serve-host 0.0.0.0`） | LAN の頁 | 言語を選ぶ口が無い | その頁の `.pair-picker` が 0 個（loopback の頁は 1 個）。`#pair-a`、`#pair-b`、`#pair-apply` も 0 個。LAN の頁の `.lang-badge` で今の組が分かる（specified：BR-31 の陰性対照と設計 10.13.13 の 12） | 陰性 | `a_page_opened_over_the_lan_has_no_language_picker` |
| T-BR-31-005 | 実走：001 の run の間、LAN の address で開いた別の頁 | 別の頁にも効く | 組は process の物 | LAN の頁にも `.pair-divider` が 1 つ出る（derived：設計 10.13.12「組は…process が終わるまで効く」。組の持ち主は `hayamimi` 1 つ） | 陽性 | `the_pair_change_reaches_a_page_opened_from_another_address` |
| T-BR-31-006 | 実走：`localStorage` の `left` が `"ja"` の頁で、`en,ja` から `ja,en` へ順序だけを変える | 順序だけの変更 | 区切りを入れない、`left` を書き換えない | `langs` が `["ja","en"]` で 1 件流れ、`.pair-divider` が 0 個、`localStorage` の `left` の値が変更の前後で同じ文字列（specified：設計 10.13.13 の 7） | 陰性 | `an_order_only_change_adds_no_divider_and_leaves_the_remembered_side` |

**BR-32（明るい表示と暗い表示。`native/hayamimi/tests/br32.rs`、純関数は `dashboard.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-32-001 | 受入、実走：窓 1280 × 800、`Emulation.setEmulatedMedia` で `prefers-color-scheme` を `dark` にして `/dashboard` を開く。switch を click、focus を置いて Enter、Space。`/transcript` も開く | 暗い、明るい | token の組どおり | 開いた頁で `html` の `data-theme` が `dark`、switch の `aria-checked` が `true`、`.dash` の `backgroundColor` が `rgb(13, 15, 19)`、`.row-text` の `color` が `rgb(236, 229, 216)`。click の後 `light`、`false`、`rgb(247, 244, 238)`、`rgb(26, 28, 33)`、`.tr-text` の `color` が `rgb(74, 77, 85)`。Enter で暗い表示に戻り、Space で明るい表示になる。`/transcript` も同じ組の色（specified：BR-32） | 陽性 | `br32_the_theme_switch_turns_the_computed_colours_into_the_other_token_set` |
| T-BR-32-002 | 純関数：表示から token の組を返す関数に `dark` と `light` | 2 つの組 | 設計の値 | `dark` は `--ink` `#0d0f13`、`--ink-2` `#14171d`、`--paper` `#ece5d8`、`--paper-dim` `#b0a898`、`--paper-faint` `#8a8473`。`light` は `#f7f4ee`、`#ece7dd`、`#1a1c21`、`#4a4d55`、`#62656d`（specified：設計 10.13.11 の表の値。`--line` ほかの明るい表示の値は designer の板で、この行に含めない） | 陽性 | `each_theme_names_its_five_fixed_token_values` |
| T-BR-32-003 | 実走：switch を 2 回押す | 往復 | 元に戻る | `data-theme`、`aria-checked`、`.dash` の `backgroundColor` が押す前と同じ（specified：BR-32 の陽性対照） | 陽性 | `two_presses_of_the_theme_switch_return_the_colours` |
| T-BR-32-004 | 実走：switch の属性 | 要素の形 | 設計の 4 つ | `role` が `switch`、`tabindex` が `0`、`aria-checked` が `"true"` か `"false"`、`aria-label` が `strings.en.toml` の文言と等しい（specified：設計 10.13.11 の switch の段） | 陽性 | `the_theme_switch_has_the_role_state_focus_and_label_the_design_names` |
| T-BR-32-005 | 実走：Enter を 1 回、別に Space を 1 回 | key の起動 | 1 回の押しで 1 回 | どちらも 1 回だけ反転する | 境界 | `enter_and_space_each_turn_the_theme_once` |
| T-BR-32-006 | 実走：OBS の字幕頁 `/` を、`dashboard` で表示を明るくした後と暗くした後に開く | 対象の外の頁 | 透明のまま | `body` の `backgroundColor` が `rgba(0, 0, 0, 0)`（specified：BR-32 の陰性対照） | 陰性 | `the_obs_caption_page_stays_transparent_in_both_themes` |
| T-BR-32-007 | 実走：窓 600 × 900 の switch | 狭い画面 | 隠さない | 矩形の幅と高さが 0 でなく、`display` が `none` でない（specified：設計 10.13.11「狭い画面でも隠さない」） | 境界 | `the_theme_switch_stays_visible_on_a_narrow_page` |

**BR-33（選んだ表示を覚える。`native/hayamimi/tests/br32.rs`、純関数は `dashboard.rs`）**

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-33-001 | 受入、実走：`prefers-color-scheme` が `dark` で明るい表示を選び、頁を読み直す | 覚えた値が端末に勝つ | 明るい | 読み直した頁の `data-theme` が `light`（specified：BR-33） | 陽性 | `br33_the_theme_is_remembered_and_an_unreadable_one_follows_the_device` |
| T-BR-33-002 | 純関数：表示を読む関数に `"light"`、`"dark"`、key の無い値、`"blue"`、`""`、`1`、`null`、`"LIGHT"` | 読める 2 つと読めない値 | 2 つだけ | `"light"` は明るい、`"dark"` は暗い、残りの 6 つは「端末に従う」（specified：BR-33。大文字は derived） | 境界 | `only_light_and_dark_are_read_and_anything_else_follows_the_device` |
| T-BR-33-003 | 実走：覚えた値が無く、`prefers-color-scheme` が `light` | 端末が明るい | 明るい | 開いた頁の `data-theme` が `light`（specified：BR-33 の陽性対照） | 陽性 | `with_nothing_remembered_a_light_device_gets_the_light_theme` |
| T-BR-33-004 | 実走：覚えた値が無い頁で、`prefers-color-scheme` を `dark` から `light` に変える | 端末の変更に従う | 読み直さずに変わる | 読み直さずに `data-theme` が `light` になる（specified：BR-33） | 陽性 | `a_device_change_is_followed_while_nothing_is_remembered` |
| T-BR-33-005 | 実走：switch を 1 回押した後で、`prefers-color-scheme` を変える | 覚えた後は従わない | 動かない | `data-theme` が押した後のまま（specified：設計 10.13.11「覚えた値が無い間は…にも従う」の反対側） | 陰性 | `after_a_choice_the_device_change_is_no_longer_followed` |
| T-BR-33-006 | 実走：`localStorage` の値を `"blue"` にして、`prefers-color-scheme` が `dark` の頁を開く | 読めない値 | 端末に従う、書き換えない | `data-theme` が `dark`、`localStorage` の値が `"blue"` のまま（specified：BR-33 の陰性対照） | 陰性 | `an_unreadable_theme_follows_the_device_and_is_not_rewritten` |
| T-BR-33-007 | 実走：`Storage.prototype.getItem` が例外を投げる頁 | 使えない `localStorage` | 端末に従う、止まらない | 端末の設定で描かれ、`Runtime.exceptionThrown` が 0 件（specified：BR-33 の陰性対照） | 陰性 | `a_throwing_local_storage_follows_the_device_and_the_page_keeps_running` |
| T-BR-33-008 | 実走：switch を押した直後の `localStorage` | 書く値 | `"light"` か `"dark"` | 明るくした直後は `"light"`、暗くし直した直後は `"dark"`（specified：設計 10.13.11「覚える」） | 陽性 | `the_chosen_theme_is_written_as_light_or_dark` |
| T-BR-33-009 | 実走：`Storage.prototype.setItem` が例外を投げる頁で switch を押す | 書けない | その頁の memory の中でだけ効く | switch を 2 回押して、`data-theme` が押すたびに切り替わり、`Runtime.exceptionThrown` が 0 件、頁を読み直すと端末の設定で描かれる。key `hayamimi.dashboard.theme` を含む `warning` が 2 回の押しを通して 1 件だけ（specified：設計 10.13.13 の 15） | 境界 | `a_theme_that_cannot_be_stored_still_switches_and_raises_nothing` |

**BR-34（字幕の文字のコントラスト。`native/hayamimi/tests/br32.rs`）**

コントラストは設計 10.13.13 の 11 の式で測る。文字の実の alpha は、文字の色の alpha に要素と祖先の computed の `opacity` をすべて掛けた値。背景は、alpha が 1 の最初の祖先の色を土台に、内側の半透明の背景を外側から順に source-over で重ねた色（sRGB の 0 から 255 の値のまま、丸めない）。文字はその上に実の alpha で重ねる。alpha が 1 の背景を持つ祖先が無い時だけ `Err`。Listening の欄が静まった時の `opacity: .5`（`css.rs:155`）も、この式で測る（T-BR-34-011）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-34-001 | 受入、実走：2 つの表示のそれぞれで、`.row-text`、`.tr-text`、`.refine-text`、`.strip-text` の `color` と背景の色の比を、設計 10.13.13 の 11 の式（文字の alpha に要素と祖先の `opacity` を掛け、背景は alpha が 1 の祖先の色を土台に半透明の背景を外側から重ねる）で計算する。静まっていない状態で測る | 2 つの表示 × 4 つの文字 | 4.5 以上 | 8 つの比がどれも 4.5 以上。要素が 1 つも見つからない文字（8 つのどれか）があれば `Err`（0 件を通と読まない。清書の段落が出る run にする）（specified：BR-34。比は純関数で計算する） | 陽性 | `br34_subtitle_text_keeps_aa_contrast_in_both_themes` |
| T-BR-34-002 | 純関数：比の関数に `#ece5d8` と `#0d0f13` | 設計の値 | 15.31 ± 0.01 | 15.31（specified：BR-34 の陽性対照。独立に組んだ `awk` の式で 15.31 と確かめた） | 陽性 | `the_contrast_of_paper_on_ink_in_the_dark_theme_is_15_31` |
| T-BR-34-003 | 純関数：設計 10.13.11 の表の残りの比、(`#b0a898`, `#0d0f13`) 8.13、(`#b0a898`, `#14171d`) 7.61、(`#1a1c21`, `#f7f4ee`) 15.53、(`#4a4d55`, `#f7f4ee`) 7.70、(`#4a4d55`, `#ece7dd`) 6.86、(`#62656d`, `#ece7dd`) 4.73 | 設計の表 | ± 0.01 | 6 つとも表の値（± 0.01） | 陽性 | `the_contrast_values_the_design_lists_are_reproduced` |
| T-BR-34-004 | 純関数：(`#767676`, `#ffffff`) と (`#777777`, `#ffffff`) を判定の関数へ | 4.5 の線の両側 | 4.5 | 前者（4.54）は `Ok`、後者（4.48）は `Err`（derived：`awk` で 2026-10-23 に計算した 4.5422 と 4.4781） | 境界 | `a_ratio_of_4_54_passes_and_4_48_fails_the_aa_line` |
| T-BR-34-005 | 純関数：(`#777777`, `#888888`) を判定の関数へ | 低い比 | `Err` | `Err`（比 1.26）（specified：BR-34 の陰性対照） | 陰性 | `a_pair_of_close_greys_fails_the_aa_line` |
| T-BR-34-006 | 純関数：比の関数に引数の順を替えた 2 通り、(`#000000`, `#ffffff`)、同じ色 2 つ | 対称、両端 | 順に依らない、21 と 1 | 順を替えても同じ比、21.00、1.00（property：式は相対輝度の式で、実装の外） | property | `the_ratio_does_not_depend_on_the_order_and_spans_one_to_twenty_one` |
| T-BR-34-007 | 純関数：文字 `#ffffff`（alpha 1）、要素の背景 `rgba(255,255,255,0.5)`、祖先の背景 `#000000`（alpha 1） | 半透明の背景 | 土台に重ねる | 背景は (127.5, 127.5, 127.5)、比は 3.98（± 0.01）。`Err` にしない（specified：設計 10.13.13 の 11。derived：`awk` で 2026-10-23 に 3.9767） | 境界 | `a_translucent_background_is_laid_over_the_opaque_ancestor` |
| T-BR-34-008 | 純関数：文字 `rgba(255,255,255,0.5)`、背景 `#000000`（alpha 1） | 半透明の文字 | 文字の alpha を背景に重ねる | 文字の実の色は (127.5, 127.5, 127.5)、比は 5.28（± 0.01）（derived：`awk` で 5.2808） | 境界 | `a_translucent_text_colour_is_laid_over_the_background` |
| T-BR-34-009 | 純関数：文字 `#ffffff`（alpha 1）、背景 `#000000`、要素の `opacity` が 0.5 と、要素 0.5 で祖先 0.5 | `opacity` の積 | alpha に掛ける | 前者は文字の実の alpha が 0.5 で比 5.28、後者は 0.25 で比 2.02（± 0.01）（derived：`awk` で 5.2808 と 2.0175。祖先の `opacity` も掛ける） | 境界 | `the_opacity_of_the_element_and_its_ancestors_multiplies_the_text_alpha` |
| T-BR-34-010 | 純関数：要素と祖先のどの背景も alpha が 1 でない（透明か半透明だけ） | 土台が無い | `Err` | `Err`（頁の外の既定の白に頼らない）。007 の入力から土台の `#000000` を透明にした物がこの入力（specified：設計 10.13.13 の 11。007 が土台の在る陽性対照） | 陰性 | `a_chain_with_no_opaque_background_is_an_error` |
| T-BR-34-011 | 実走：2 つの表示のそれぞれで、`.strip` に `quiet` が付いた状態（最後の `final` の後）の `.strip-text` を測る | Listening の欄が静まった状態 | 4.5 以上 | `color` が暗い表示で `rgb(138, 132, 115)`、明るい表示で `rgb(98, 101, 109)`（`--paper-quiet`）。`.strip-text` と祖先の computed の `opacity` がすべて 1。背景は `--ink-2` で、比が暗い表示 4.81、明るい表示 4.73（± 0.01）で、どちらも 4.5 以上（specified：設計 10.13.13 の 11 と 13。derived：`awk` で 2026-10-23 に 4.8107 と 4.7315）。この行は、`opacity: .5` と `--paper-dim` の今の style（比 2.86 と 2.37）では落ちる。落ちる間も線は緩めない | 陰性 | `the_quiet_listening_strip_keeps_aa_contrast_in_both_themes` |
| T-BR-34-012 | 実走：同じ 2 つの表示で、静まった状態と静まっていない状態の `.strip-text` の `fontWeight` と、`.strip` の見出し（`.strip-label`）を含む行の text を比べる | 静まった印は色だけでない | 太さ 400、印の語 | 静まった状態の `.strip-text` の `fontWeight` が `400`、静まっていない状態は `600`。静まった状態の見出しの行の text に、静まっていない状態に無い語が増え、その語が `strings.en.toml` か `strings.ja.toml` の値の 1 つと等しい（specified：設計 10.13.13 の 13「文字の太さ 400」と「印の語」。語の key は設計が決めず、toml の値との一致で測る） | 陽性、陰性 | `the_quiet_strip_is_marked_by_weight_and_a_word_and_not_by_colour_alone` |
| T-BR-34-013 | 純関数：比の関数に (`#8a8473`, `#14171d`) と (`#62656d`, `#ece7dd`) | `--paper-quiet` と欄の背景 | 4.5 以上 | 4.81 と 4.73（± 0.01）で、どちらも判定の関数が `Ok`（specified：設計 10.13.13 の 13。陰性対照は 005 の近い灰色） | 陽性 | `the_quiet_token_pairs_pass_the_aa_line` |

**BR-35（制御の口を LAN に開かない。`native/hayamimi/tests/br35.rs`、純関数は `native/hayamimi/src/routes.rs` の `mod tests`）**

材料：`--serve-host 0.0.0.0` で起こし（port は OS に選ばせる）、LAN の address は test が UDP の socket の local address で引く（RP-40 と同じ。loopback の外の address が引けない時は `Err`）。相手の address は client の socket の bind で決める（LAN の相手は LAN の address に bind して `<LAN の address>:<port>` へ、loopback の相手は `127.0.0.1:<port>` へ）。制御の口の 5 つは `POST /config`、`GET /config`、`POST /reset`、`POST /replacements`、`POST /itn_overrides`（受入）に、`GET /replacements`、`GET /itn_overrides`（設計 10.13.6「POST だけでなく GET も同じ」）を足した 7 つである。POST の body は loopback の相手が打って 200 になる物（実装の席が `api.rs` の受け付ける形から作る fixture）で、403 が body の不備による 400 と混ざらないようにする。「状態が変わらない」は、loopback の相手の `GET /config`、`GET /replacements`、`GET /itn_overrides` の応答の body が、LAN の相手の打った後も打つ前と同じであることで測る。`/events` の頭は、応答の頭を大文字小文字を区別せずに探す。model は要らない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-35-001 | 受入：LAN の相手から `POST /config`、`GET /config`、`POST /reset`、`POST /replacements`、`POST /itn_overrides`。打つ前後に loopback の相手が 3 つの GET で状態を読む | 制御の口 × LAN の相手 | 403 | 5 本とも status が 403（400 でも 503 でもない）。3 つの GET の body が前後で同じ（specified：BR-35。設計 10.13.6 の C6） | 陰性 | `br35_the_control_ports_refuse_a_lan_peer_and_leave_the_state_alone` |
| T-BR-35-002 | LAN の相手から `GET /replacements`、`GET /itn_overrides` | 読む口 | 403 | 2 本とも 403 で、body に設定の中身（loopback の相手が読んだ body の文字列）が含まれない。loopback の相手の同じ 2 本は 200（specified：設計 10.13.6「`GET /config` は設定を読ませる」。003 の loopback の相手が対照） | 陰性 | `a_lan_peer_cannot_read_the_replacements_or_the_overrides` |
| T-BR-35-003 | loopback の相手から 7 つの口を、001 と同じ body で打つ。続けて 3 つの GET で読む | loopback の相手 | 今のとおり受ける | `POST /config`、`POST /replacements`、`POST /itn_overrides` は 200 で、続く GET が打った内容を返す。`POST /reset` の status は、この変更の前の server と同じ（200、または session が終わっている時の 503）で、403 でない。`GET` の 3 本は 200（specified：BR-35 の陽性対照） | 陽性 | `a_loopback_peer_still_uses_every_control_port` |
| T-BR-35-004 | 純関数：相手が loopback かを返す関数に `127.0.0.1`、`127.0.0.0`、`127.255.255.255`、`::1`、`::ffff:127.0.0.1`、`126.255.255.255`、`128.0.0.0`、`192.168.1.5`、`::ffff:192.168.1.5`、`::`、`fe80::1`、`0.0.0.0` | loopback の範囲の両端と外 | `127.0.0.0/8` と `::1` | 前の 5 つが真、残りの 7 つが偽（specified：設計 10.13.13 の 6「loopback は `127.0.0.0/8` と `::1`」「`::ffff:127.0.0.1` からの `POST /config` は loopback として受ける」） | 境界 | `the_loopback_judgement_covers_the_whole_127_range_and_the_wrapped_form` |
| T-BR-35-005 | `GET /events` を LAN の相手と loopback の相手がそれぞれ開く（`--serve-host 0.0.0.0`） | `/events` の頭 | 頭が無い | 2 本とも 200 で content type が `text/event-stream`、`access-control-allow-origin` の頭が無い（specified：BR-35。設計 10.13.6「`--serve-host` が loopback の外の時は `/events` の応答から `access-control-allow-origin` を外す」） | 陰性 | `the_events_reply_has_no_cors_header_when_the_serve_host_is_not_loopback` |
| T-BR-35-006 | `--serve-host` を省く（既定の `127.0.0.1`）で起こし、`GET /events` と 7 つの口を loopback の相手が打つ | 既定の組 | 今のまま | `/events` が `access-control-allow-origin: *` を持ち、7 つの口は 003 と同じに受けられる（specified：BR-35 の陰性対照。設計 10.13.6「loopback の時は今と何も変えない」） | 陽性 | `with_the_default_serve_host_nothing_changes` |
| T-BR-35-007 | `--serve-host` が (a) `127.0.0.2`、(b) `::1` の run | loopback の別の書き方 | loopback の時は変えない | 2 本とも `/events` が `access-control-allow-origin: *` を持つ（`127.0.0.1` に繋いだ時の振る舞いと同じ。(b) は IPv6 の loopback に繋げない機械では `Err`）（specified：設計 10.13.13 の 6 の loopback の範囲。006 の対照） | 境界 | `any_loopback_serve_host_keeps_the_cors_header` |
| T-BR-35-008 | LAN の相手から `GET /`、`GET /dashboard`、`GET /transcript`、`GET /ui/` 下の頁が読む file 1 つ | 制御の口の外 | 今のとおり | 4 本とも 200（specified：設計 10.13.6「頁の振る舞いは変わらない」。001 の 403 が制御の口に限られることの対照。RP-40 と同じ） | 陽性 | `the_pages_and_files_are_still_served_to_a_lan_peer` |
| T-BR-35-009 | LAN の相手が `POST /config` を、`Host: localhost`、`X-Forwarded-For: 127.0.0.1`、`Origin: http://127.0.0.1:<port>` を付けて打つ | 名乗りの偽り | 相手の address だけで決める | 403 で、`GET /config` の body が前後で同じ（specified：設計 10.13.6「相手の address が loopback の時だけ受け」。003 の loopback の相手が対照） | 陰性 | `a_lan_peer_cannot_pass_as_loopback_by_headers` |
| T-BR-35-010 | 中継の hub（`hayamimi_relay::hub::bind_on` と `serve`）を `0.0.0.0` で起こし、LAN の相手が `GET /events` を打つ | 中継は C6 の外 | 今のとおり | 200 で content type が `text/event-stream`、`access-control-allow-origin: *` を持つ（specified：設計 10.13.6 の C6 と BR-35「応答は今のとおり `access-control-allow-origin: *` を持つ」、H69。005 の `hayamimi` の `/events` に頭が無いことの対照で、頭を探す関数が頭の在る応答を見つけられること） | 陽性 | `the_relay_events_reply_keeps_the_cors_header` |

**BR-36（持った文を、区間を閉じた無音の中で読み終えている時に使う。`native/hayamimi-asr/tests/br36.rs`、旗の行は `native/hayamimi/src/args.rs` の `mod tests`、run の記録を読む行は `native/hayamimi/tests/br03.rs`）**

材料：`hayamimi_asr::endpoint` の `State<&str>`、`Reach`、`reach_for`、`closing_text`、`note_plan`、`Decision` を公開の API から使う。標本は 16 kHz で、1 窓（`WINDOW`）は 512 標本（32 ms）、`silence` の閉じの無音は 4800 標本（300 ms）。区間は 1000 から 20000 まで（`seg_start`、`seg_end`）。持った文は `hold("held", range_end, 19000)`（第 3 引数が `floor`）。`Decision` は規則と無音を持ち、`silence` は 300 ms、`max_speech` と `flush` は 0 ms とする。`held_gap_ms` は、読んだ範囲の終わりが区間の終わりより後なら正の整数の ms で、標本から ms への四捨五入は絶対値に掛ける（半分は切り上げ）。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-36-001 | 受入：閉じが `silence`（300 ms）、`range_end` が 21920（`seg_end` の 1920 標本後。早い終わりの通知が置く 120 ms の無音）、持った後に声なし。`covering` と `strict` のそれぞれで `plan_with_gap` を呼び、`note_plan` に通す | 持った文の読みが区間の終わりを越えて、閉じた無音の中に終わる | 上の限り 4800 標本（300 ms） | `covering` は `Plan::Held`（文は `held`、start は 1000）で `held_gap_ms` が `None`。`strict` は `Plan::Whole` で `held_gap_ms` が `Some(120)`（specified：設計 10.13.4 の手 9。IP-4 の記録の読み直し 28 件の多くが 44〜148 ms） | 陽性 | `br36_a_held_sentence_that_covers_the_closing_silence_is_used_as_the_final` |
| T-BR-36-002 | 001 の入力で `range_end` を 19488（`seg_end` の 512 標本前）にする | 下の限りの内側 | 下の限り 512 標本（1 窓） | 2 つの旗とも `Plan::Held`（specified：今の幅。`strict` の振る舞いが変わらないことの対照） | 境界 | `a_held_sentence_read_one_window_short_is_used_by_both_flags` |
| T-BR-36-003 | 001 の入力で `range_end` を 19487（513 標本前）にする | `strict` の下の限りの外側、`covering` の下の限りの内側 | `strict` は 512 標本（1 窓） | `covering` は `Plan::Held`、`strict` は `Plan::Whole` で `held_gap_ms` が `Some(-32)`（513 標本は −32.06 ms）（specified：読み終わりが区間の終わりの手前でも、その後ろが閉じた無音の中なら、区間は声を欠かない） | 境界 | `a_held_sentence_read_more_than_one_window_short_is_used_by_covering` |
| T-BR-36-004 | 001 の入力で `range_end` を 24800（`seg_end` の 4800 標本後）にする | 上の限りの内側 | 4800 標本 | `covering` は `Plan::Held`、`strict` は `Plan::Whole` で `Some(300)` | 境界 | `a_held_sentence_read_to_the_end_of_the_closing_silence_is_used_by_covering` |
| T-BR-36-005 | 001 の入力で `range_end` を 24801（4801 標本後）にする | 上の限りの外側 | 4800 標本 | 2 つの旗とも `Plan::Whole`、`Some(300)`（4801 標本は 300.06 ms） | 境界 | `a_held_sentence_read_past_the_closing_silence_is_read_again_by_both_flags` |
| T-BR-36-006 | 001 の入力で、持った後に `note_voice(19000)`（`floor` 以上の声の始まり）を呼ぶ | 持った文の後ろに声が在る | 声が在れば使わない | 2 つの旗とも `Plan::Whole`、`Some(120)`（specified：設計 10.13.4 の手 9「その後ろに声が無ければ」） | 陰性 | `a_held_sentence_with_voice_after_it_is_never_used` |
| T-BR-36-007 | 001 の入力で、閉じの規則を (a) `max_speech`（無音 0 ms）、(b) `flush`（無音 0 ms）にする | 無音を持たない閉じ | 前の限りと後ろの限りが 512 標本 | (a) と (b) のどちらも、`covering` でも `Plan::Whole` で `Some(120)`。`range_end` を 19487（513 標本前）にした持った文も、(a) と (b) のどちらでも `Plan::Whole` で `Some(-32)`（specified：設計 10.13.4 の手 9「`max_speech` と `flush` の閉じには無音が無いので、前と後ろの限りは今の 1 窓」） | 陰性 | `a_close_without_silence_gets_no_wider_reach` |
| T-BR-36-008 | 線が在る形：`confirm(5000, 4900)` で線（`at` 5000、`floor` 4900）を置き、`note_voice(5200)` を呼び、`hold("held", 21920, 19000)` し、001 と同じ閉じで区間 1000 から 20000 を呼ぶ | 線の後ろの持った文 | 線の `at` が区間の中（5000 は 1000 より後） | `covering` は `Plan::Held(_, 5000)`（start は線の位置）、`strict` は `Plan::Rest(5000)`（specified：`plan_with_gap` の線の枝。持った文は線から先を読んだ物） | 陽性 | `a_held_sentence_after_a_line_is_used_from_the_line_by_covering` |
| T-BR-36-009 | 純関数：`reach_for` に (`Covering`、`silence` 300 ms)、(`Covering`、`max_speech`)、(`Covering`、`flush`)、(`Strict`、`silence` 300 ms)、規則が `max_speech`、`flush`、`early` で `silence_ms` が 300 の `Decision` に (`Covering`) | 旗と閉じの組 | 設計の手 9 | 順に `Reach { behind: 4800, ahead: 4800 }`、`Reach { behind: 512, ahead: 512 }`、`Reach { behind: 512, ahead: 512 }`、`Reach { behind: 512, ahead: 512 }`。無音を 300 ms 持つ 3 組も `Reach { behind: 512, ahead: 512 }`（specified：設計 10.13.4 の手 9「規則 `silence` で閉じた区間に限り」。幅を決めるのは規則で、`silence_ms` の値ではない） | 陽性、陰性 | `the_reach_of_each_flag_and_close_is_the_one_in_the_design` |
| T-BR-36-010 | 純関数：`note_plan` に (`Plan::Whole`、差 +1920 標本)、(`Plan::Whole`、差 −513 標本)、(`Plan::Whole`、差 `None`)、(`Plan::Held`、差 +1920 標本)、(`Plan::Rest`、差 +1920 標本) | 符号つきの ms | 絶対値の四捨五入 | 順に `Some(120)`、`Some(-32)`、`None`、`None`（使った時は残さない）、`Some(120)`（specified：設計 10.13.4 の手 9「`held_gap_ms` は、読んだ範囲の終わりが区間の終わりより後なら正」。1920 標本は 120 ms ちょうど） | 境界 | `the_held_gap_is_a_signed_number_of_milliseconds` |
| T-BR-36-011 | 純関数：引数の読み。(a) 旗なし、(b) `--held-reuse covering`、(c) `--held-reuse strict`、(d) `--held-reuse maybe`、(e) `--held-reuse` の値なし | 旗の値 | `strict` と `covering` の 2 つ | (a) と (b) は `HeldReuse::Covering`、(c) は `HeldReuse::Strict`。(d) は `ArgError::HeldReuseUnsupported`（key は `held_reuse_unsupported`）、(e) は値の無い旗のエラー（`--partial-lid` の同じ形の行と同じ）（specified：設計 10.13.9 の IP-S17「既定は `covering`」） | 陽性 | `held_reuse_takes_strict_or_covering_and_nothing_else` |
| T-BR-36-012 | 実走：BR-3 の harness が、旗を付けない起動（`covering`）で `lat-dialog` を `--repeat 3` で流して記録する（step は `br03_ip_s17`） | 既定の run | 記録が揃う | `br03_ip_s17.md`、`.jsonl`、`.events.jsonl`、`.log.jsonl` が書かれ、`.log.jsonl` に `asr.endpoint.decided`、`asr.endpoint.held`、`asr.utterance.timed`、`asr.feed.waited` の行が在る（specified：設計 10.13.9 の IP-S17「記録の道具が、これらの行も残す」） | 陽性 | `the_run_with_covering_held_reuse_is_recorded` |
| T-BR-36-013 | 実走：同じ harness が `--held-reuse strict` を付けて流して記録する（step は `br03_ip_s17_strict`） | 旗を外した run | 記録が揃う | 012 と同じ 4 つの file が書かれる（specified：設計 10.13.9 の IP-S17「戻し方は `--held-reuse strict`」。012 と同じ build で続けて取る） | 陽性 | `the_run_with_strict_held_reuse_is_recorded` |
| T-BR-36-014 | 純関数：run の log を読む記録の関数に、合成の 3 行（いずれも `asr.endpoint.decided` で `rule` が `silence`）を渡す。(a) `plan` が `decode` で `held_gap_ms` が 120、(b) `plan` が `decode` で `held_gap_ms` が −40、(c) `plan` が `held` | 記録の読み | 符号 | 記録の関数が、`silence` の閉じの件数、`plan`（`held`、`decode`、`rest`）ごとの件数、読み直した件の `held_gap_ms` の符号ごとの件数を出す。3 行では `silence` が 3 件、`decode` が 2 件（正が 1 件、負が 1 件）、`held` が 1 件。名指しされない `plan` と、`plan` の無い `silence` の閉じは `Err`（specified：設計 10.13.10 の H72「符号が読みどおりか」を実走の記録で確かめるための数え方） | 陽性 | `the_held_plan_counts_and_signed_gaps_of_a_run_are_counted_from_its_log` |
| T-BR-36-015 | 実物：012 と 013 の 2 本の記録（`docs/results/in-person/br03_ip_s17*`）を読む | 旗の前後 | 確定文の割り方が同じで、text が違う確定文は持った文を使った件だけ | `type == "final"` の `[id, lang]` が 2 本で全件同じ。text は文末の `.`、`。`、読点（`,`、`;`、`:`、`、`、`，`）を除いて比べ、違う確定文は全部、012 の `asr.endpoint.decided` で `plan` が `held`、013 で `held` でない件である（`id` の番号で結ぶ）。`silence` の閉じで `plan` が `held` の件数が、012 の方が 013 より多い。手で作った 2 組の確定文で、違いが持った文の使い回しで説明できる組は空を返し、両方が読み直した組と両方が持った組は違う確定文の id を返し、文末が `b.` と `b,` だけ違う組は plan によらず空を返す（specified：設計 10.13.7 の BR-36。早い終わりの読みと区間全体の読みは音の範囲が違うので text は同じとは限らず、違いは持った文を使った件に限る。文末の記号は、持った文を使った件で `closing_text` が替えるので比べない。同じ件数を通と読まないために「多い」を求める。違った件数は設計の穴 H72 に書く） | 陽性、陰性 | `the_two_held_reuse_runs_differ_only_in_the_finals_covering_made_from_kept_sentences` |
| T-BR-36-016 | 001 の入力で `range_end` を (a) 17568（`seg_end` の 2432 標本前。早い終わりの通知が置く無音 1920 標本と 1 窓の和）、(b) 15200（4800 標本前）にする | 前の限りの内側 | 前の限り 4800 標本（300 ms） | (a) と (b) のどちらも、`covering` は `Plan::Held`（start は 1000）、`strict` は `Plan::Whole` で差が `Gap::Short`（`seg_end` から `range_end` を引いた標本数）（specified：設計 10.13.4 の手 9） | 境界 | `a_held_sentence_read_short_inside_the_closing_silence_is_used_by_covering` |
| T-BR-36-017 | 001 の入力で `range_end` を 15199（`seg_end` の 4801 標本前）にする | 前の限りの外側 | 前の限り 4800 標本 | 2 つの旗とも `Plan::Whole`、`held_gap_ms` は `Some(-300)`（4801 標本は −300.06 ms） | 境界 | `a_held_sentence_read_short_past_the_closing_silence_is_read_again_by_both_flags` |
| T-BR-36-018 | 純関数：`closing_text` に (`Covering`、`silence`) の閉じで、(`en`、`He would have to pay her the money,`)、(`en`、`Done, then; `)、(`en`、`It is raining.`)、(`en`、`A, B`)、(`en`、空)、(`ja`、`雨が降っていますが、`)、(`ja`、`雨が降っています。`)、(`zh`、`你好，`)。ほかに (`Strict`、`silence`、`en`、`Done,`) と、(`Covering`、`max_speech` と `flush`、`en`、`Done,`) | 持った文の文末の記号 | 語と記号 | 順に `He would have to pay her the money.`、`Done, then.`、`It is raining.`、`A, B`、空、`雨が降っていますが。`、`雨が降っています。`、`你好，`。ほかの 3 組は `Done,` のまま（specified：設計 10.13.4 の手 9。閉じが無音なら文は終わっていて、区間全体の読みは文末に終端の記号を置く。実走で、`covering` が使った持った文の文末の `,` が、`strict` の読み直しの `.` と違う件が 4 つ出た） | 陽性、陰性 | `a_kept_sentence_closed_by_silence_ends_with_the_terminal_mark_of_its_language` |
| T-BR-36-019 | 実物と純関数：012 の記録（`br03_ip_s17.log.jsonl` と `br03_ip_s17.events.jsonl`）を読む。先に、手で作った `asr.endpoint.decided` の 5 行（番号 1、2、5 が `silence` で `held`、3 が `silence` で `decode`、4 が `max_speech` で `held`）と 5 件の確定文（1 は英語で `.` 終わり、2 は英語で `, ` 終わり、3 は英語で `,` 終わり、4 は日本語で `、` 終わり、5 は `zh`）で、記録を読む 2 つの関数を確かめる | `silence` の閉じで持った文から作った確定文 | 文末（空白を除く）の記号 | 手で作った組では、持った文の番号が 1、2、5 と出て、そのうち英語と日本語の 2 件（1 と 2）を調べ、文末が読点の 1 件（`file-2`）だけが返る。`file-<番号>` の形でない id と、番号の無い `asr.endpoint.decided` は `Err`。記録では、`silence` の閉じで `plan` が `held` の件が 1 件以上在り、その件数と、番号で結んだ英語と日本語の確定文の件数が等しく、その確定文の全部が文末に `,`、`;`、`:`、`、`、`，` を持たない（specified：設計 10.13.4 の手 9「閉じが `silence` なら、文末の読点を終端の記号に替える」を、`closing_text` を呼ぶ `Engine::on_segment` まで含めて実走の記録で確かめる。`on_segment` は model を要して単体では呼べないので、呼び出しが外れれば、次に記録を取り直した時にこの行が落ちる） | 陽性、陰性 | `the_finals_covering_made_from_kept_sentences_end_with_no_pause_mark` |

**BR-37（確定文の言語を、声が 4 秒に届いた時の途中の判定で決める。`native/hayamimi-asr/tests/br37.rs`、定数の行は `native/hayamimi-asr/src/partial.rs` の `mod tests`、旗の行は `native/hayamimi/src/args.rs` の `mod tests`、run の記録を読む行は `native/hayamimi/tests/br03.rs`）**

材料：`hayamimi_asr::partial` の `MidwayLid`、`MidwayAsked`、`Midway`、`MIDWAY_SAMPLES`、`MIDWAY_CLOSING_SAMPLES`、`CLOSING_ALIGN_SAMPLES`、`hayamimi_asr::audio` の `History` と `WINDOW`、`hayamimi_asr::route` の `Router`、`Policy`、`Request`、`hayamimi_asr::lid` の `Identify` と `lid_clip` を公開の API から使う（BR-4 の test と同じ呼び方）。標本は 16 kHz で、1 窓（`WINDOW`）は 512 標本（32 ms）、2 秒は 32 000 標本、4 秒は 64 000 標本（125 窓）。識別器と認識器は台本で、識別器は問われた音を控え、答えを test が決める。窓を `History` に入れてから、窓ごとに `observe` を 1 回呼ぶ。`speech_start` は声の始まりの絶対の位置で、前置きの無音を入れた test では 0 でない。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-37-001 | 受入：pair の `Policy`（`final_midway` が真）の `Router` と `MidwayLid`。識別器は問われた音の長さを控え、`ja` と答える。`speech_start` を 0 にして 125 窓（64 000 標本）を入れ、窓ごとに `observe` を呼ぶ。続けて `closing_for(0)` の答えを `Request` の `midway_lang` に載せ、音が 64 000 標本、`speech_s` が 4.0 の `Request` で `transcribe` を呼ぶ | 声が 4 秒に届いた区間 | 64 000 標本（4 秒） | 識別器への問いは合計 2 回（63 窓目の 2 秒の問いと、125 窓目の 4 秒の問い）で、`transcribe` は識別器に訊かない。`Transcript` の `lang` が `ja`、`lid_ms` が 0、`fallback` が `None`。125 窓目の `MidwayAsked` は `voice_samples` が 64 000、`answer` が `Heard("ja")`（specified：設計 10.13.4 の手 12） | 陽性 | `br37_a_midway_answer_at_four_seconds_names_the_final_language_without_a_second_question` |
| T-BR-37-002 | 001 の入力で、120 窓目から 130 窓目までの各窓の `observe` の返りを読む | 4 秒の手前と後 | 125 窓 | 124 窓目までは 4 秒の問いの返りが無く、125 窓目に 1 つ、126 窓目から 130 窓目までは返りが無い。識別器への問いは、63 窓目から数えて合計 2 回のまま（specified：設計 10.13.4 の手 12「声が 4 秒に届いた時」。手 1 の 2 秒の問いと同じく、1 発話に 1 度） | 境界 | `the_four_second_question_is_asked_once_when_the_voice_reaches_four_seconds` |
| T-BR-37-003 | 001 の入力で、(a) `speech_start` を 20 000 にして、先に 20 000 標本の無音を `History` に入れてから声を 125 窓入れる、(b) `speech_start` を 5 000 にして、先に 5 000 標本の無音を入れてから声を 125 窓入れる | 声の始まりの前の音 | 前置き 16 000 標本（1 秒） | 4 秒の問いの `voice_samples` が、(a) 16 000 + 64 000 = 80 000、(b) 5 000 + 64 000 = 69 000（前置きは声の始まりまでの 5 000 標本に切られる）。識別器が受けた音の長さも同じ（specified：設計 10.13.4 の手 12「声の始まりの前の最大 1 秒を含む音」。`History::draft` の前置きの扱い） | 境界 | `the_four_second_question_carries_up_to_one_second_before_the_voice` |
| T-BR-37-004 | 001 の声を、4 つの policy で入れる。(a) pair で `final_midway` が偽（`partial_lid` は真）、(b) pair で `final_midway` が真、`partial_lid` が偽、(c) `Policy::fast()`、(d) `Policy::single("ja")` | 旗の組と mode | 4 つの policy | (a) は 2 秒の問いだけが 1 回（63 窓目）で、`closing_for(0)` が `None`。(b) は 4 秒の問いだけが 1 回（125 窓目）で、`closing_for(0)` が `Some("ja")`、`heard()` が `None`。(c) と (d) は問いが 0 回で、`closing_for` が `None`（specified：設計 10.13.4 の手 12 の旗 `--final-lid close` が (a) の形。手 1 の `--partial-lid` とは別々に効く） | 陰性 | `the_four_second_question_is_asked_only_by_a_pair_run_that_asks_for_it` |
| T-BR-37-005 | 2 秒の問いと 4 秒の問いが同じ呼びで要る形。001 の policy で、`History` に 130 窓を一度に入れてから `observe` を 1 回だけ呼ぶ | 2 つの問いが同時に要る | 同じ呼びで 1 回 | 識別器への問いが 1 回で、`closing_for(0)` が `Some("ja")`、`heard()` も `Some("ja")`（2 つの欄に同じ答えが置かれる）（specified：設計 10.13.4 の手 12。識別器の 1 回の問いは約 80〜110 ms（H73）かかり、同じ呼びで 2 回は訊かない） | 境界 | `the_two_questions_due_in_one_call_cost_one_question` |
| T-BR-37-006 | 識別器の答え。(a) 2 秒にも 4 秒にも `en`、(b) 4 秒の問いに `zh`（2 つの外）、(c) 4 秒の問いに失敗、(d) 2 秒の問いに `zh`、4 秒の問いに `ja` | 答えの種類 | `ja`、`en`、2 つの外、失敗 | (a) は `closing_for(0)` が `Some("en")`。(b) と (c) は `None`。(d) は `Some("ja")` で、`heard()`（2 秒の欄）は `None`（specified：設計 10.13.4 の手 12「2 つの外の答えは今のとおり落ち先の連鎖を通る」。2 つの欄は別々の答えを持つ） | 陰性 | `a_midway_answer_outside_the_pair_or_a_failure_is_not_used_for_the_final` |
| T-BR-37-007 | 位置。001 の policy で `speech_start` を 20 000 にして答えを引いた後、`closing_for(from)` を `from` が 20 000、12 000、11 999、20 001 で呼ぶ | 声の始まりと区間の始まりの差（声の始まり − 区間の始まり） | 8 000 標本（0.5 秒） | 差が 0 と 8 000 のとき（`from` が 20 000 と 12 000）は `Some("ja")`。差が 8 001 のとき（11 999）と −1 のとき（20 001）は `None`（specified：設計 10.13.4 の手 12「区間の始まり以上 0.5 秒（8 000 標本）以内」） | 境界 | `the_answer_is_used_only_for_a_segment_that_starts_just_before_the_voice` |
| T-BR-37-008 | 新しい発話。001 の policy で答えを引いた後、(a) `speech_start` を `None` にして `observe` を呼ぶ。(b) 続けて `History` の終わり（64 000）を新しい始まりにして `observe` を呼び、そこから声を 125 窓入れ直す | 声の始まりの変化 | 始まりが変われば忘れる | (a) の直後に `closing_for(0)` が `None`。(b) の呼びは問いを出さず、直後に `closing_for(<新しい始まり>)` が `None`。入れ直した声が 125 窓に届くまで 4 秒の問いは出ず、届いた窓で新しい答えが 1 回出る。識別器への問いは全体で 4 回（specified：`MidwayLid` は声の始まりが変わると答えを忘れる（手 1 と同じ）。古い答えを次の発話に使わない） | 陰性 | `a_new_voice_start_forgets_the_answer_and_asks_again_at_four_seconds` |
| T-BR-37-009 | `Router::transcribe` の `Request`。pair の `Policy`、識別器は常に `en` と答えて問われた回数を数え、`speech_s` は 6.0、`live` は真。(a) `midway_lang` が `ja`、(b) `en`、(c) `zh`、(d) `None` | 載った答えの種類 | `ja`、`en`、その外、無し | (a) は `lang` が `ja` で識別器が 0 回、(b) は `lang` が `en` で 0 回、どちらも `lid_ms` が 0。(c) と (d) は識別器が 1 回訊かれ、`lang` は識別器の答えの `en`（specified：設計 10.13.4 の手 12「答えが `ja` か `en` なら省く」。`known_lang` の振る舞いは変えない） | 陽性と陰性 | `the_final_asks_the_identifier_unless_a_midway_answer_names_a_language_of_the_pair` |
| T-BR-37-010 | 同じ音。`History` に 16 384 標本（32 窓）の無音を入れ、その後ろに声（定数 0.5）を 134 窓入れる。声の始まりの検出が 4 608 標本（9 窓、約 0.3 秒）遅れた形で、`speech_start` を 20 992 にする。4 秒の問いで識別器が受けた音と、区間（始まり 16 384）を `with_preroll` した音を確定の側が識別器に渡した音を、どちらも `lid_clip` に通して比べる | 検出の遅れ | 遅れ 4 608 標本 | 2 つの clip の長さがどちらも 64 000 標本で、全標本が等しい（specified：設計 H73「検出した声の始まりは区間の始まりより約 0.3 秒後」。手 12 は 2 つの clip を同じにする） | 境界 | `the_four_second_question_and_the_final_read_the_same_clip` |
| T-BR-37-011 | 純関数：引数の読み。(a) 旗なし、(b) `--final-lid midway`、(c) `--final-lid close`、(d) `--final-lid maybe`、(e) `--final-lid` の値なし。(a) から (c) を `--mode pair` と `--mode fast` のそれぞれで読み、`policy()` の `final_midway` を見る | 旗の値と mode | `midway` と `close` の 2 つ | pair は (a) と (b) が真、(c) が偽。fast は (a) から (c) のすべてが偽。(d) は `ArgError::FinalLidUnsupported`（key は `final_lid_unsupported`）、(e) は値の無い旗のエラー（`--held-reuse` の同じ形の行と同じ）（specified：設計 10.13.9 の IP-S20「既定は `midway`」） | 陽性 | `final_lid_takes_midway_or_close_and_nothing_else_and_asks_nothing_outside_a_pair_run` |
| T-BR-37-012 | 実走：BR-3 の harness が、旗を付けない起動（`midway`）で `lat-dialog` を `--repeat 3` で流して記録する（step は `br03_ip_s20`） | 既定の run | 記録が揃う | `br03_ip_s20.md`、`.jsonl`、`.events.jsonl`、`.log.jsonl` が書かれ、`.log.jsonl` に `asr.partial.lang.asked` と `asr.utterance.timed` の行が在る（specified：設計 10.13.9 の IP-S20「記録：確定文の `id`、`lang`、`text` が `close` の run と全件同じ」） | 陽性 | `the_run_with_the_final_language_decided_midway_is_recorded` |
| T-BR-37-013 | 実走：同じ harness が `--final-lid close` を付けて流して記録する（step は `br03_ip_s20_close`） | 旗を外した run | 記録が揃う | 012 と同じ 4 つの file が書かれる（specified：012 の対照の run） | 陽性 | `the_run_with_the_final_language_decided_at_the_close_is_recorded` |
| T-BR-37-014 | 実物と純関数：012 と 013 の記録（`docs/results/in-person/br03_ip_s20*`）を読む。先に、手で作った記録で数える関数を確かめる | 旗の前後 | 確定文の `id`、`lang`、`text` | 手で作った記録：`asr.utterance.timed` の `lid_ms` が 0 の行を 2 つ持つ記録で、数える関数がその 2 つの番号を返し、持たない記録で空を返し、`id` か `lid_ms` の無い行は誤りにする。`asr.partial.lang.asked` の `voice_ms` が 2 016、3 999、4 000、5 200 の行を持つ記録で 4 000 以上を 2 件と数える（境界は 3 999 と 4 000）。確定文の並びが 1 件違う 2 本は違う `id` を、長さの違う 2 本は余りの `id` を名指す。実物：(1) `type == "final"` の `[id, lang, text]` の並びが 2 本で全件同じ、(2) `lid_ms` が 0 の確定文の数が `midway` の run で 1 件以上、`close` の run で 0 件、(3) `midway` の run の `asr.partial.lang.asked` のうち `voice_ms` が 4 000 以上の行が 1 件以上で、(2) の数以上、(4) `close` の run にその行が 0 件（specified：設計 10.13.9 の IP-S20、10.13.10 の H73。実走の前の記録では (2) と (4) の数が 0 になる） | 陽性と陰性 | `the_midway_and_close_runs_name_the_same_finals_and_only_midway_skips_the_question` |
| T-BR-37-015 | 純関数：定数 | 4 秒の問いの長さ | `LID_MAX_SECONDS` | `MIDWAY_CLOSING_SAMPLES` が 64 000 で、`LID_MAX_SECONDS` に標本率 16 000 を掛けた値と等しく、`CLOSING_ALIGN_SAMPLES` が 8 000（specified：設計 10.13.4 の手 12。確定の側の判定が読む長さと同じにする） | 境界 | `the_four_second_limits_match_what_the_final_identifier_reads` |

**BR-38（速報の復号を、専用の thread の recognizer で回す。`native/hayamimi-asr/tests/br38.rs`、旗の行は `native/hayamimi/src/args.rs` の `mod tests`、log の行は `native/hayamimi/src/log.rs` の `mod tests`、言語の表の行は `native/hayamimi/src/strings.rs` の `mod tests`、run の記録を読む行は `native/hayamimi/tests/br03.rs`）**

材料：`hayamimi_asr::draft` の `Drafter`、`DraftReader`、`DraftNote`、`DraftRead`、`hayamimi_asr::partial` の `DraftClock`、`hayamimi_asr::audio` の `History`、`hayamimi_asr::route` の `Tier`、`hayamimi_asr::error` の `AsrError` を公開の API から使う。標本は 16 kHz で、速報の周期の 0.5 秒は 8 000 標本、音の上限の 8 秒は 128 000 標本。reader は台本で、呼ばれた thread の id、受けた tier と音を控え、答え（文、失敗、panic）を呼ばれた順に返す。台本の `prepare` と `read` は、test の手で開く門を待てる。待ちは全部、上限つき（5 秒）で、上限を過ぎたら test を落とす。時計は `DraftClock::observe(true, 0)` で声の始まりを 0 にし、窓の代わりに `History::push` で標本を足してから `step` を呼ぶ。`step` の最後の引数（worker を持たない時に読む reader）には、worker を持つ `Drafter` では読まれない台本を渡し、読まれたら数える。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-38-001 | 受入：`Drafter::start` に台本の reader（`read` は `"one"` と答える）を渡し、`wait_ready` で準備を待つ。時計は声の始まりを 0 にし、`History` に 8 000 標本を入れ、`step`（tier は `Tier::Rz`）を呼ぶ。台本が読み終えるのを待ち、160 標本を足して `step` をもう 1 回呼ぶ | 周期の最初の速報 | 8 000 標本（0.5 秒） | 1 回目の `step` は `Ok(None)` で、reader が `read` を呼んだ thread の id は test の thread と違う。2 回目の `step` は `Ok(Some("one"))`。worker を持たない時の reader は 1 回も読まれない（specified：設計 10.13.4 の手 11。頼んだ呼びは文を返さず、後の呼びが返す） | 陽性 | `br38_a_draft_is_read_by_a_worker_of_its_own_and_published_by_a_later_call` |
| T-BR-38-002 | 台本の `prepare` が門を待つ。(a) 門が閉じたまま、`History` の終わりを 8 000 にして `step` を呼ぶ。(b) 門を開き、`wait_ready` が真を返してから、標本を足さずに `step` を呼ぶ | 準備の前後 | 準備が済むまで | (a) は `Ok(None)` で台本の `read` は 0 回。(b) の呼びで `read` が 1 回呼ばれ、音は 8 000 標本（specified：設計 10.13.4 の手 11「準備が済むまで頼みを受けない。周期の目盛りは使わない」。目盛りを (a) で使っていれば、(b) の同じ位置では周期が来ず、頼みが出ない） | 陰性 | `a_worker_takes_no_job_before_it_is_ready_and_the_due_tick_is_kept` |
| T-BR-38-003 | 台本の `read` が門を待つ。(a) 終わり 8 000 で `step`（頼みが出る）。(b) 終わりを 16 000 にして `step`。(c) 門を開き、台本が読み終えるのを待って、標本を足さずに `step` | 読んでいる間 | 周期 1 つ分の音（8 000 標本）が (a) の後に来ている | (b) は `Ok(None)` で、台本が受けた頼みは 1 件のまま。(c) は `Ok(Some(a の文))` を返し、同じ呼びで 2 件目の頼みが出て、その音は `History::draft(0, 128 000)` と等しく 16 000 標本である（specified：設計の手 11「読んでいる間は次を頼まず、目盛りを使わない」「読み終わった次の呼びがすぐ頼む」） | 境界 | `a_busy_worker_gets_no_second_job_and_the_next_call_hands_the_latest_audio` |
| T-BR-38-004 | 003 の (a) の後、台本が読んでいる間に、終わりを 16 000 にして `restart_at(4_000)` で声の始まりを動かし、台本が読み終えてから `step`。2 件目の答えは `"two"` | 声の始まりの移動 | 始まりが 0 から 4 000 | 読み終えた後の `step` は `Ok(None)` で、1 件目の文は使われない。同じ呼びで、始まり 4 000 からの頼みが出る。台本が 2 件目を読み終えた後の `step` は `Ok(Some("two"))`。`take_notes` の `Read` は、1 件目が `used` 偽、2 件目が `used` 真（specified：設計の手 11「声の始まりが今と同じ時だけ文にする」。早い終わりの確定で始まりが動いた後に、確定した文を覆う速報を出さない） | 陰性 | `a_result_whose_speech_start_moved_is_dropped` |
| T-BR-38-005 | 003 の (a) の後、台本が読んでいる間に `observe(false, 12_000)` で声を終わらせ、台本が読み終えてから `step` | 声の終わり | 声の外 | `Ok(None)`。頼みは出ない（台本の `read` は 1 回のまま）。`Read` は `used` 偽の 1 件（specified：設計の手 11。確定文が出た後に、その発話の速報を出さない） | 陰性 | `a_result_after_the_speech_ended_is_dropped` |
| T-BR-38-006 | 003 の (a) の後、台本が読んでいる間に、終わりを 16 000 にして `Drafter::reset()` を呼ぶ。声の始まりは同じ。(a) reset の直後に `step`、(b) 台本が読み終えてから `step` | session の世代 | 世代が 1 つ進む | (a) は `Ok(None)` で頼みを出さない（worker は読み中）。(b) は `Ok(None)` で、`Read` は `used` 偽の 1 件。同じ呼びで新しい頼みが出る（specified：設計の手 11「前の世代の結果を使わない」。読み中の worker は結果が着くまで空かない） | 陰性 | `a_result_of_an_earlier_session_is_dropped_and_the_worker_stays_busy_until_it_ends` |
| T-BR-38-007 | 台本の答えが、1 件目は `Err(AsrError::NoResult)`、2 件目は `"two"`。終わり 8 000 で頼み、読み終えるのを待って終わりを 16 000 にし、`step`。続けて 2 件目を読み終えるのを待ち、`step` | 読みの失敗 | 1 件目だけ失敗 | 1 回目の `step` は `Err(AsrError::NoResult)`（`matches!`）で、同じ呼びで 2 件目の頼みが出る。2 回目の `step` は `Ok(Some("two"))`。`take_notes` の `Read` は、1 件目が `used` 偽、2 件目が `used` 真で、`Lost` は無い（specified：設計の手 11「読みの失敗は失敗として返り、worker は次の頼みを読む」。失敗が周期を止めない。失敗した読みは使った読みに数えない） | 陰性 | `a_failed_read_is_returned_by_the_later_call_and_the_next_job_is_read` |
| T-BR-38-008 | 007 と同じ流れで、1 件目の読みが panic する台本 | panic | 1 件目だけ | 007 と同じ形で、1 回目の `step` は `Err(_)`、2 回目は `Ok(Some("two"))`。`take_notes` の `Read` は、1 件目が `used` 偽、2 件目が `used` 真で、`Lost` は無い（specified：設計の手 11「panic は失敗として返り、worker は次の頼みを読む」。`Refiner` の reader の panic の扱いと同じ） | 陰性 | `a_reader_that_panics_fails_its_job_and_not_the_worker` |
| T-BR-38-009 | 声 20 秒（320 000 標本）を `History` に入れ、声の始まり 0、tier `Tier::V3` で `step` を呼んで頼みを出す。台本が受けた音と tier を読む。続けて tier を `Tier::Rz` にして `step`（結果を受ける） | 頼みの内容 | 音の上限 128 000 標本 | 台本が受けた音は `History::draft(0, 128 000)` と標本まで等しく、長さは 128 000。tier は頼んだ時の `Tier::V3`（specified：設計の手 11「頼みの音は声の始まりの前の最大 1 秒から今までで、最後の 8 秒まで。読む tier は頼んだ時に決めた物」） | 境界 | `a_job_carries_the_audio_the_inline_read_would_take_and_the_tier_given_when_it_was_handed` |
| T-BR-38-010 | `Drafter::inline()`。声の始まり 0、`History` の終わり 8 000。`step` の reader は台本で、呼ばれた thread の id と音を控え、`"one"` と答える | worker を持たない形 | 今の形 | `step` は同じ呼びで `Ok(Some("one"))`。reader が呼ばれた thread は test の thread。音は `History::draft(0, 128 000)` と等しい。`Read` は `worker` 偽で、`wait_ms` が `decode_ms` と等しい（specified：設計の手 11「`--draft-worker off` は今の形」） | 陽性 | `without_a_worker_a_due_draft_is_read_in_the_same_call_on_the_callers_thread` |
| T-BR-38-011 | 台本の `prepare` が panic して worker の thread が終わる。`wait_ready` が偽を返してから、終わり 8 000 で `step`（worker を持たない時の reader は `"inline"` と答える台本）。続けて終わり 16 000 で `step` | 終わった worker | thread が終わった | 1 回目の `step` は同じ呼びで `Ok(Some("inline"))`。`take_notes` は `Lost` をちょうど 1 件返す。2 回目も inline で読まれ、`Lost` は増えない（specified：設計の BR-38 の行「worker の thread が終わった時は、その後の速報を loop が読み、終わったことを 1 度だけ log に残す」） | 陰性 | `a_worker_that_ended_is_replaced_by_the_inline_read_and_said_once` |
| T-BR-38-012 | worker を起こし、`read` を 1 回通した後、`close` を 2 回呼ぶ。その後、終わり 16 000 で `step`（inline の reader は `"inline"`） | 閉じる | 2 回 | 台本の `release` は 1 回で、`release` を呼んだ thread は `read` を呼んだ thread と同じで test の thread と違う。`close` の戻りの時点で、`release` の後に台本が立てた印が立っている。`close` の後の `step` は `Ok(Some("inline"))` で、`Lost` は返らない（specified：設計の手 11。run の終わりに worker が残らない） | 境界 | `closing_joins_the_worker_releases_the_reader_once_and_leaves_the_inline_read` |
| T-BR-38-013 | 台本の `prepare` と `read` がそれぞれ 20 ms 眠る。声の始まり 0、終わり 16 000 で頼みを 1 件出し、答えを使う。`take_notes` を 2 回呼ぶ | 記録 | 1 件の速報 | 1 回目は先頭に `Ready`（`ready_ms` が 20 以上）、続いて `Read` が 1 件（`worker` 真、`used` 真、`audio_ms` が 1 000、`decode_ms` が 20 以上、`wait_ms` が `decode_ms` 以上）。2 回目は空（specified：設計の手 11 の log の 1 行 `asr.partial.read`。`audio_ms` は音の標本数を 16 で割った値、`wait_ms` は頼んでから使うまでの実時間） | 陽性 | `the_notes_name_the_ready_time_and_each_read_with_its_times` |
| T-BR-38-014 | (a) 台本の `prepare` が何もしない。`wait_ready(5 秒)`。(b) `prepare` が門を待つ。`wait_ready(50 ms)`、門を開いて `wait_ready(5 秒)` | 準備の待ち | 上限 50 ms | (a) は真。(b) の最初は偽で、戻るまでの実時間は 50 ms 以上 2 秒未満、門を開いた後は真（specified：設計の手 11「`warm_up` が準備を上限つきで待つ」。準備が済まなくても run は止まらない） | 境界 | `waiting_for_the_worker_is_bounded_and_ends_when_it_is_ready` |
| T-BR-38-015 | 声の始まり 0 の時計で、`History` の終わりを 4 000、8 000、12 000、16 000 と進め、各回で `step` を呼ぶ。台本は即座に答える。(a) `Drafter::inline()`。(b) worker（各回の後に読み終わりを待つ） | 周期の規則 | 8 000 標本ごと | どちらも、読み（頼み）が出るのは終わりが 8 000 と 16 000 の回だけで、音の長さは 8 000 と 16 000。(b) の文は頼んだ回の次の回に返る（8 000 の頼みの文は 12 000 の回に返る）（specified：`DraftClock::due` の規則は 2 つの形で同じ） | 陽性と境界 | `the_due_rule_of_the_clock_is_the_same_with_and_without_a_worker` |
| T-BR-38-016 | 純関数：引数の読み。(a) 旗なし、(b) `--draft-worker on`、(c) `--draft-worker off`、(d) `--draft-worker maybe`、(e) `--draft-worker` の値なし、(f) `--draft-worker on --no-partial` | 旗の値 | `on` と `off` の 2 つ | (a) と (c) は worker を使わず、(b) は使う。(d) は `ArgError::DraftWorkerUnsupported`（key は `draft_worker_unsupported`）、(e) は値の無い旗のエラー（`--final-lid` の同じ形の行と同じ）。(f) は速報が無いので worker を使わない（specified：設計 10.13.9 の IP-S19「既定は `off`」） | 陽性と陰性 | `draft_worker_takes_on_or_off_and_nothing_else_and_starts_no_worker_without_drafts` |
| T-BR-38-017 | 純関数：log の行。`partial_read_line`、`draft_ready_line`、`draft_lost_line` に値を渡し、続けて `draft_note_line` に同じ値の 3 つの note を渡す | 3 つの event | event 名 | `asr.partial.read` は info で `worker`、`used`、`audio_ms`、`decode_ms`、`wait_ms` を持つ。`asr.draft.ready` は info で `ready_ms` を持つ。`asr.draft.lost` は warn で `error_code` が `HAYAMIMI_ASR_DRAFT_LOST`。3 行とも速報の文字を持たない。`draft_note_line` は 3 つの note を、同じ値から作った 3 行と同じ文字列に写す（specified：設計 10.13.4 の手 11。log に本文を出さない決め） | 陽性 | `the_draft_lines_carry_their_fields_and_no_text` |
| T-BR-38-018 | 実走：BR-3 の harness が `--draft-worker on` を付けて `lat-dialog` を `--repeat 3` で流して記録する（step は `br03_ip_s19`） | `on` の run | 記録が揃う | `br03_ip_s19.md`、`.jsonl`、`.events.jsonl`、`.log.jsonl` が書かれ、`.log.jsonl` に `asr.partial.read` と `asr.draft.ready` の行が在る（specified：設計 10.13.9 の IP-S19「記録：BR-3 の run の段ごとの値」） | 陽性 | `the_run_with_the_draft_worker_on_is_recorded` |
| T-BR-38-019 | 実走：同じ harness が `--draft-worker off` を付けて流して記録する（step は `br03_ip_s19_off`） | `off` の run | 記録が揃う | 018 と同じ 4 つの file が書かれ、`.log.jsonl` に `asr.partial.read` の行が在る（`worker` は偽）（specified：018 の対照の run） | 陽性 | `the_run_with_the_draft_worker_off_is_recorded` |
| T-BR-38-020 | 実物と純関数：018 と 019 の記録（`docs/results/in-person/br03_ip_s19*`）と、その前に流した組の複写（`br03_ip_s19_run1*` と `br03_ip_s19_off_run1*`。どの実走も書き直さない名前）を読む。先に、手で作った記録で数える関数を確かめる | 旗の前後 | 確定文の `id`、`lang`、`text` | 手で作った記録：`worker` が真の `asr.partial.read` を 2 行持つ記録で 2 と数え、持たない記録で 0 と数え、`worker` の無い行は誤りにする。確定文の並びが 1 件違う 2 本は違う `id` を、長さの違う 2 本は余りの `id` を名指す。実物は 2 つの組のそれぞれで、(1) `type == "final"` の `[id, lang, text]` の並びが 2 本で全件同じ、(2) `asr.partial.read` の `worker` が真の行が `on` の run に 1 件以上、`off` の run に 0 件、(3) `asr.draft.ready` が `on` の run に 1 件、`off` の run に 0 件、(4) `asr.draft.lost` が `on` の run に 0 件。加えて (5) 2 つの組の確定文の並びが全件同じ（specified：設計 10.13.9 の IP-S19、10.13.10 の H77。実走の前の記録では (2) と (3) の数が 0 になる） | 陽性と陰性 | `the_on_and_off_runs_name_the_same_finals_and_only_on_reads_by_the_worker` |
| T-BR-38-021 | 純関数：言語の表。`strings.en.toml` と `strings.ja.toml` から `asr.partial.read`、`asr.draft.ready`、`asr.draft.lost` の message を引く | 2 つの言語 | event 3 つ | 6 つの message がどれも空でなく、2 つの言語で違う文である（specified：log の文言を locale で切り替える決め） | 陽性 | `the_draft_lines_have_a_message_in_both_languages` |
| T-BR-38-022 | 台本の `prepare` が panic して worker の thread が終わる。`wait_ready` を呼ばず、`History` の終わりを 8 000 にして、`step`（worker を持たない時の reader は `"inline"` と答える台本）を、`take_notes` に `Lost` が出る呼びまで繰り返す（呼びの間は 1 ms 眠り、上限 10 秒） | `step` の中で見つかる終わった worker | `wait_ready` が終わりを見つけない | `Lost` が出た呼びは、同じ呼びで `Ok(Some("inline"))` を返し、`take_notes` は `Lost` をちょうど 1 件返す。その前の呼びは `Ok(None)` で、reader は呼ばれない。reader が呼ばれた回数は 1 回、音は 8 000 標本、thread は test の thread。`has_worker` は偽（specified：設計の BR-38 の行「worker の thread が終わった時は、その後の速報を loop が読み、終わったことを 1 度だけ log に残す」。011 は `wait_ready` が終わりを見つける順で、これは `step` が見つける順である。見つけた呼びで読まなければ、その呼びの速報が 1 回欠ける） | 陰性 | `a_worker_found_gone_by_the_step_itself_is_replaced_by_the_inline_read_in_that_call` |

**BR-39（認識器の thread 数を引数で決める。旗の行は `native/hayamimi/src/args.rs` の `mod tests`、言語の表の行は `native/hayamimi/src/strings.rs` の `mod tests`、設定から認識器の集まりへの行は `native/hayamimi-asr/src/pipeline.rs` の `mod tests`、run の記録の行は `native/hayamimi/tests/br03.rs`）**

材料：`hayamimi::args::parse` に引数の並びを渡し、`Args` の `asr_threads` と `ArgError` を読む。`hayamimi_asr::pipeline` の `Settings::new` が返す設定の `asr_threads` を変え、非公開の `recognizers_of` と `worker_reader` が作る物の `threads` を読む（model は読まない）。実走は BR-3 の harness が `--asr-threads` を付けて `lat-dialog` を `--repeat 3` で流し、記録を `docs/results/in-person/` に書く。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-39-001 | 純関数：`--wav a` に `--asr-threads` を付けない行と、`1`、`4`、`8`、`32` を付けた行を `parse` に渡す | 既定と範囲の中 | 1 から 32 | 付けない行は `asr_threads` が `hayamimi_asr::recognizer::THREADS`（4）である。付けた行は渡した数と等しい（specified：設計 10.13.4 の手 10。`--asr-threads 4` で今の形） | 境界 | `asr_threads_starts_at_the_recognizer_default_and_takes_one_to_thirty_two` |
| T-BR-39-002 | 純関数：`--asr-threads` に `0`、`33`、`-1`、`8.5`、空、`four`、`2147483648` を付けた行と、値の無い行を `parse` に渡す | 範囲の外と、数でない値 | 1 から 32 | 値のある行は `ArgError::AsrThreadsUnsupported`、値の無い行は `ArgError::MissingValue` である。`AsrThreadsUnsupported` の key は `asr_threads_unsupported`（specified：同） | 陰性 | `asr_threads_refuses_what_is_not_a_whole_number_from_one_to_thirty_two` |
| T-BR-39-003 | 純関数：`Settings::new` が返す設定と、`asr_threads` を 8 にした設定から `recognizers_of` で認識器の集まりを作り、`threads` を読む | 既定と指定 | 4 と 8 | 既定の設定は `asr_threads` が `THREADS`（4）で、作った集まりの `threads` も 4 である。8 にした設定からは 8 である（specified：設計 10.13.9 の IP-S18「`pipeline.rs` の `Settings` の `asr_threads` と `recognizers_of`」。集まりを作るだけで model は読まない） | 境界 | `the_recognizers_of_a_run_are_opened_with_the_thread_count_of_its_settings` |
| T-BR-39-004 | 純関数：`asr_threads` を 8 にした設定から `worker_reader` で速報の worker の reader を作り、`threads` を読む | 速報の worker | 8 | 8 である（specified：設計 10.13.4 の手 10。経路の認識器と同じ数で、worker の認識器だけが 4 のまま残らない） | 陰性 | `the_draft_worker_opens_its_recognizers_with_the_same_thread_count` |
| T-BR-39-005 | 純関数：言語の表。`strings.en.toml` と `strings.ja.toml` から `help` の文と `asr_threads_unsupported` の文を引く | 2 つの言語 | 文 2 つ | どちらの `help` も `--asr-threads <n>` を含み、拒否の文は `--asr-threads` と `32` を含み、2 つの言語で違う文である（specified：command line の文言を locale で切り替える決め） | 陽性 | `the_asr_threads_help_line_and_refusal_are_in_both_languages` |
| T-BR-39-006 | 実走：BR-3 の harness が `--asr-threads 8` を付けて `lat-dialog` を `--repeat 3` で流して記録する（step は `br03_ip_s18`） | 8 thread の run | 記録が揃う | `br03_ip_s18.md`、`.jsonl`、`.events.jsonl`、`.log.jsonl` が書かれる（specified：設計 10.13.9 の IP-S18「記録：BR-3 の run の確定文の `decode_ms`」） | 陽性 | `the_run_with_eight_asr_threads_is_recorded` |
| T-BR-39-007 | 実走：同じ harness が `--asr-threads 4` を付けて流して記録する（step は `br03_ip_s18_four`） | 4 thread の run | 記録が揃う | 006 と同じ 4 つの file が書かれる（specified：006 の対照の run） | 陽性 | `the_run_with_four_asr_threads_is_recorded` |

**BR-40（日本語の速報に確定と同じ ITN と句読点を当てる。旗の行は `native/hayamimi/src/args.rs` の `mod tests`、言語の表の行は `native/hayamimi/src/strings.rs` の `mod tests`、速報の後処理の行は `native/hayamimi-asr/src/pipeline.rs` の `mod tests`、run の記録の行は `native/hayamimi/tests/br03.rs`）**

材料：`hayamimi::args::parse` に引数の並びを渡し、`Args` の `draft_marks` と `ArgError` を読む。`hayamimi_asr::pipeline` の非公開の `draft_wears_marks` に旗と tier を渡す。005 は句読点の model を持つ `Engine`（fetch 済みの model を読むので `#[ignore]`）の非公開の `draft_text` に文と tier を渡す。実走は BR-3 の harness が `--draft-marks` を付けて `lat-dialog` を `--repeat 3` で流し、記録を `docs/results/in-person/` に書く。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-BR-40-001 | 純関数：`--wav a` に `--draft-marks` を付けない行と、`on`、`off` を付けた行を `parse` に渡す | 既定と 2 つの値 | on と off | 付けない行と `off` の行は `draft_marks` が偽、`on` の行は真である（specified：設計 10.13.4 の手 13。`--draft-marks off` で今の形） | 境界 | `draft_marks_starts_off_and_takes_on_or_off` |
| T-BR-40-002 | 純関数：`--draft-marks` に `maybe`、空、`ON`、`1`、`true` を付けた行と、値の無い行を `parse` に渡す | on と off の外 | on と off | 値のある行は `ArgError::DraftMarksUnsupported`、値の無い行は `ArgError::MissingValue` である。`DraftMarksUnsupported` の key は `draft_marks_unsupported`（specified：同） | 陰性 | `draft_marks_refuses_what_is_not_on_or_off` |
| T-BR-40-003 | 純関数：言語の表。`strings.en.toml` と `strings.ja.toml` から `help` の文と `draft_marks_unsupported` の文を引く | 2 つの言語 | 文 2 つ | どちらの `help` も `--draft-marks <on\|off>` を含み、拒否の文は `--draft-marks` と `off` を含み、2 つの言語で違う文である（specified：command line の文言を locale で切り替える決め） | 陽性 | `the_draft_marks_help_line_and_refusal_are_in_both_languages` |
| T-BR-40-004 | 純関数：`draft_wears_marks` に、旗が真と偽、tier が `Tier::Rz` と、それ以外の全部（`Tier::ALL`）を渡す | 旗と tier | `Rz` だけ | 旗が真で tier が `Rz` の時だけ真である。旗が偽なら `Rz` でも偽で、旗が真でも `Rz` 以外は全部偽である（specified：設計 10.13.4 の手 13「他の tier の速報は変えない」） | 陰性 | `a_draft_takes_the_marks_of_a_final_only_from_the_japanese_tier_with_the_setting_on` |
| T-BR-40-005 | `#[ignore]`：句読点の model を持つ 2 つの `Engine`（旗が偽と真）に、句読点の無い日本語の文 `けさの東京風はなさそうですけれどもね体感はどうでしょうか近藤さん` を `Tier::Rz` の速報として `draft_text` に渡し、真の側には英語の文 `HELLO THERE` も `Tier::V3` として渡す | 旗の 2 値と tier | 句読点が付く | 偽の側は文のまま返る。真の側は文と違い、`？` か `。` を含む。`V3` の文は変わらない（specified：同。辞書の置換はどちらの側も通る） | 陽性 | `a_japanese_draft_has_the_marks_of_a_final_with_the_setting_on_and_none_with_it_off` |
| T-BR-40-006 | 実走：BR-3 の harness が `--draft-marks on` を付けて `lat-dialog` を `--repeat 3` で流して記録する（step は `br03_ip_s21`） | on の run | 記録が揃う | `br03_ip_s21.md`、`.jsonl`、`.events.jsonl`、`.log.jsonl` が書かれる（specified：設計 10.13.9 の IP-S21「記録：`translate.draft.reused` の数」） | 陽性 | `the_run_with_draft_marks_on_is_recorded` |
| T-BR-40-007 | 実走：同じ harness が `--draft-marks off` を付けて流して記録する（step は `br03_ip_s21_off`） | off の run | 記録が揃う | 006 と同じ 4 つの file が書かれる（specified：006 の対照の run） | 陽性 | `the_run_with_draft_marks_off_is_recorded` |

**設計の在り処**（この席は受入の条件を足さない。行は次の節を引く）：

- 段ごとの遅れ、冷えた標本、線：設計 10.13.3（T-BR-01 から T-BR-03）。
- 速くする手の 1 と 2：10.13.4（T-BR-04、T-BR-05）。log の 1 行：10.13.4 の手 3（T-BR-06）。n-gram の記録：手 5（T-BR-07）。
- 質の手 Q-a から Q-i：10.13.5（T-BR-08 から T-BR-15）。
- 別の PC のブラウザから音を送る頁：10.13.6（T-BR-16 から T-BR-18）。制御の口を LAN に開かない C6：10.13.6 の末尾（T-BR-35）。
- 持った文を使う幅：10.13.4 の手 9（T-BR-36）。
- 確定文の言語を途中の判定で決める：10.13.4 の手 12（T-BR-37）。
- 速報の復号を専用の thread で回す：10.13.4 の手 11（T-BR-38）。
- 認識器の thread 数を引数で決める：10.13.4 の手 10（T-BR-39）。既定を 8 にする条件は H83。
- 日本語の速報に確定と同じ ITN と句読点を当てる：10.13.4 の手 13（T-BR-40）。既定を `on` にする条件は H84。
- 3 つの枠、境目、入れ替え、字体と大きさ：10.13.11（T-BR-19 から T-BR-25）。H62 の決めは T-BR-24 の前書きに書いた。
- 言語の組：10.13.12（T-BR-26 から T-BR-31）。H65 の決めは T-BR-27-016 から 019。
- 明るい表示と暗い表示：10.13.11 の「明るい表示と暗い表示」（T-BR-32 から T-BR-34）。静まった欄の印は設計 10.13.13 の 13（`--paper-quiet`、太さ 400、印の語）で、T-BR-34-011 から 013 が測る。

**この節が命令で測れない述語と、測れる者**：

- 合言葉を定数時間で比べること（BR-16）：時間の差は test で安定して測れない。コードの読みで確かめる者は perfectionist-reviewer。
- Teams の tab と画面の共有の音（`getDisplayMedia`、BR-18、H53）：自動では測れない。人が手で試して記録に書く（screen-test-engineer ではなく利用者の手）。
- 窓の firewall の規則（設計 10.13.6）：test の外。手順書を書く席と利用者が測る。
- 送る側の Chrome と Edge に flag の項目が在るか、登録した origin の頁から同じ origin の `ws` が繋がるか（P0-1、P0-4）：送る側の PC の前の利用者が `docs/results/in-person/p0.md` に記録する。T-BR-18-001 と T-BR-18-004 は `chrome-headless-shell` の中だけを測る。
- LAN の外の address からの実接続（BR-16）：test の機械には LAN の外の address が無い。純関数の T-BR-16-004 から 008 が判定を決める。
- 合言葉が中継へ出ないこと（設計 10.13.6）：BR-16 の test は中継を起こさない。`hayamimi` の code に合言葉を中継へ送る経路が無いことを、perfectionist-reviewer が読みで確かめる。
- 画面の見た目（線の太さ、つまみの形、色の細部、行の左右の寄せ方の細部、H59 の空きの読みやすさ）：designer の板と、IP-S12 の後に人が開いた頁。
- Windows の 3 つの字体（`yu-gothic`、`meiryo`、`yu-mincho`）が端末で描かれる字体：機械で決まるので記録だけ（T-BR-24-014）。
- 頁の読み込みの間の一瞬の色（H67 の 1 つめ）：実装の席が測って記録する。
- `ready` の frame に足した `stream_s` を、知らない key を捨てない consumer が読んでも壊れないこと（H58）：`rg -n '"ready"' native mobile` で数えるのは IP-S6 の実装の席。T-BR-13-008 は producer の形だけを測る。
- 仮名を持たない日本語の文が `zh,ja` の組で `zh` へ送られる量（H68）：`zh,ja` を使う時の実装の席が交互の録音で数える。T-BR-27-022 は仮名の無い文を `zh` と読む規則だけを測る。
- 初回表示の字体 file の配布条件（H61）：原文の条件を読む者が確かめる。
- `pipeline.rs` が 4 秒の問いの答えを確定の読みに通す配線（`consider` と `process` の 2 か所、BR-37）：Silero の model を読む `Engine` が要り、CI では測れない。BR-3 の 2 本の run の `lid_ms` が 0 の確定文の数（T-BR-37-014）で測る。
- `Engine::partial` が `Drafter::step` を通す配線と、`Engine::open` が worker を起こして `Engine::warm_up` が準備を上限つきで待つ配線（BR-38）：認識器と Silero の model を読む `Engine` が要り、CI では測れない。BR-3 の 2 本の run の `asr.partial.read` の `worker` が真の行の数と `asr.draft.ready` の数（T-BR-38-020）で測る。
- worker が増やす memory の量と、worker の読み込みの秒数（設計 10.13.10 の H77）：run の最中に受付が測って記録する。`ready_ms` は `asr.draft.ready` の行から読む。

### 4.88 RP-96（翻訳要求の打ち直し）

test：`native/hayamimi-relay/tests/rp96.rs`、`native/hayamimi-relay/src/llm.rs` の `mod tests`、`native/hayamimi-relay/src/pipeline.rs` の `mod tests`、`native/hayamimi-relay/src/strings.rs` の `mod tests`。受入 test の名前は `rp96_a_closed_connection_is_asked_again_and_the_line_is_translated`。
行は 3 つの置き場に分かれる。001 から 003 は、live の relay を偽の recognizer と偽の llama-server で起こす行で、`rp96.rs` に置く。004 から 011 と 014 は `HttpLlm` と純関数の行で `llm.rs` の `mod tests` に、012 は `pipeline.rs` の `mod tests` に、013 は `strings.rs` の `mod tests` に置く。14 行とも model も GPU も `testdata/` も読まないので、`#[ignore]` は付けない。
command：001 から 003 は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --test rp96`。004 から 014 は `cd native && flock -w 1800 target/release/.cargo-lock true && cargo test --release --jobs 2 -p hayamimi-relay --lib` に、絞りとして `llm::tests::`、`pipeline::tests::`、`strings::tests::` を順に足す。
走る行の数：`--test rp96` に `-- --list 2>/dev/null | grep -c ': test$'` を足すと 3 になる。`--lib` に同じ `--list` を足した出力に `a_connection_closed_before_the_answer_is_asked_again_once` が 1 件在ることが、陽性対照である。
関数の名前は各行の「関数の末尾」と同じにし、何も付けない。`llm.rs` の純関数は `cause_of(error: &ureq::Error) -> &'static str` と `asks_again(error: &ureq::Error) -> bool` とする。
設計は D 3.23、D 8 の R8z、D 6 の RP-96、D 10 の H166、D 11 の S55 から S57 である。

**数え方**：

- 接続の数は、偽の server が accept した本数である。偽の server は 1 本の接続で 1 要求を読み、head と body を読み切ってから応答する。
- 「応答なしで閉じる」は、request を読み切った後に何も書かずに socket を閉じる（FIN で閉じる）ことである。ureq はこれを `Error::Io` の `UnexpectedEof`（`Peer disconnected`）で返す（ureq 3.4.2 の `run.rs`。S57）。
- 004 から 011 の log の行は、`LogSink` を実装した集め役の sink で読む。集め役は event の複製を `Arc<Mutex<Vec<LogEvent>>>` に積み、test が `SharedSink::new` で包んで `HttpLlm` に渡す。
- 「誰も聞いていない port」は、`bind` して手放した port である。
  多くの機械は接続を拒否し、`cause` は `connection_refused` になる。
  firewall が接続を捨てる機械は無応答になり、要求の時間切れで `timeout` になる。
  どちらも閉じられた接続ではないので、003 は 2 つの語のどちらかを通し、送り直さないことを検査する。
- 001 から 003 の log の行は、`tests/support/events.rs` の `Logs` で、event 名を数えて読む。`translate.fallback.held` の数も、`/events` の `fallback` の frame ではなく、log の event 名で数える。

| case | 入力 | 同値類と境界 | 閾値 | 期待値と式（oracle） | 対照 | 関数の末尾 |
|---|---|---|---|---|---|---|
| T-RP-96-001 | 受入：live の relay を偽の recognizer と偽の llama-server で起こす（`r8f_live.rs` と同じ組み立て）。server は 1 本目の接続で request を読み切った後に応答なしで閉じ、2 本目から固定の訳を返す。recognizer が確定文を 1 行送る | 閉じられた接続が 1 回起きる | 送り直しは 1 回 | `/events` に確定文の訳が `status` `ok` で 1 件。log の `translate.request.retried` が 1 行で `cause` が `unexpected_eof`、`translate.request.failed` と `translate.fallback.held` が 0 行。server の受けた接続が 2 本 | 陽性 | `rp96_a_closed_connection_is_asked_again_and_the_line_is_translated` |
| T-RP-96-002 | 001 と同じ組み立てで、server がどの接続も応答なしで閉じる | 閉じられた接続が続く | 送り直しは 1 回で止まる | 訳が `status` `held` で 1 件。`translate.request.retried` が 1 行、`translate.request.failed` が 1 行で `cause` が `unexpected_eof`。server の受けた接続が 2 本ちょうど | 境界、陰性 | `a_server_that_always_hangs_up_holds_the_line_after_one_more_try` |
| T-RP-96-003 | 001 と同じ組み立てで、`--translate zh --llm-other http://127.0.0.1:<bind して手放した port>`（誰も聞いていない port）にして確定文を送る | 接続の拒否（閉じられた接続ではない） | 送り直しは 0 回 | 訳が `status` `held` で 1 件。`translate.request.failed` が 1 行で `cause` が `connection_refused` か `timeout`、`translate.request.retried` が 0 行 | 陰性 | `a_refused_connection_is_not_asked_again` |
| T-RP-96-004 | `HttpLlm` に `logging_to` で集め役の sink を付け、1 本目を応答なしで閉じ、2 本目が固定の答えを返す偽の server へ `complete(&Completion::new("x", 8))` する | 閉じられた接続が 1 回起きる | 送り直しは 1 回 | `Ok` で固定の答え。server の受けた接続が 2 本。集めた event に `translate.request.retried` が 1 行で `cause` が `unexpected_eof` | 陽性 | `a_connection_closed_before_the_answer_is_asked_again_once` |
| T-RP-96-005 | 004 と同じで、server がどの接続も応答なしで閉じる | 閉じられた接続が続く | 送り直しは 1 回で止まる | `Err(LlmError::Transport(_))`。server の受けた接続が 2 本ちょうど。`translate.request.retried` が 1 行 | 境界、陰性 | `a_failure_is_asked_again_once_and_not_more` |
| T-RP-96-006 | 004 と同じで、server が 500 を返す | 応答のある失敗 | 送り直しは 0 回 | `Err(LlmError::Status(500))`。server の受けた接続が 1 本。`translate.request.retried` が 0 行 | 陰性 | `an_answered_error_status_is_not_asked_again` |
| T-RP-96-007 | `cause_of` に `ureq::Error::Io` の `UnexpectedEof`、`ConnectionReset`、`ConnectionAborted`、`BrokenPipe`、`ConnectionRefused`、`TimedOut`、`Other`、続けて `ureq::Error::Timeout` の `Global`、`HostNotFound`、`BadUri("x")` を順に渡す | 10 通りの分け方 | 一覧の 10 語 | 順に `unexpected_eof`、`connection_reset`、`connection_aborted`、`broken_pipe`、`connection_refused`、`timed_out`、`io_other`、`timeout`、`host_not_found`、`other` | 陽性 | `each_failure_kind_has_its_own_cause` |
| T-RP-96-008 | `asks_again` に 007 と同じ 10 の入力を順に渡す | 閉じられた接続の 4 種と、それ以外の 6 種 | 真は 4 つだけ | 先頭の 4 つが真、残りの 6 つが偽 | 境界、陰性 | `only_the_four_closed_connection_kinds_are_asked_again` |
| T-RP-96-009 | 004 と同じで、request に `Wanted`（counter が自分の世代と違い、`still()` が偽）を付ける | もう要らない要求 | 送り直しは 0 回 | `Err(LlmError::Aborted)`。server の受けた接続が 1 本。`translate.request.retried` が 0 行 | 陰性 | `a_request_not_wanted_any_more_is_not_asked_again` |
| T-RP-96-010 | 004 の server を、`Reach::new` で包んだ `HttpLlm` へ `complete` する | 救えた失敗 | `Watch::drain()` の長さ 0 | `Ok`。`drain()` が空（`Held` も `Cleared` も無い） | 陽性 | `a_retry_that_succeeds_does_not_hold_the_translator` |
| T-RP-96-011 | 004 で集めた `translate.request.retried` の event を、JSON の行にして読む | event の項目 | 該当なし | `level` が `warn`、`dependency` が `llama-server`、`duration_ms` が数、`cause` が `unexpected_eof`、`error_code` が `null`。行の文字列に `127.0.0.1` が無い | 陽性 | `the_retried_event_names_the_dependency_and_the_cause_and_no_address` |
| T-RP-96-012 | `pipeline.rs`：台本の `Llm` が `LlmError::Transport("connection_reset: io: Connection reset by peer (os error 104)")` を返す。別の台本は `LlmError::Status(503)` を返す | `Transport` と `Status` | 該当なし | `Transport` の台本の `translate.request.failed` は `detail` が `transport`、`cause` が `connection_reset`。`Status` の台本は `cause` が `null` | 陽性、陰性 | `a_failed_request_names_its_cause_and_keeps_its_detail` |
| T-RP-96-013 | `strings.rs`：`Strings::embedded` を `Lang::Ja` と `Lang::En` で作り、`message_for("translate.request.retried")` を引く | 表に行が在る | 該当なし | 2 つとも空でなく、event 名そのものでもない（行が無いと message は event 名になる） | 陽性 | `the_retried_event_has_a_message_in_both_languages` |
| T-RP-96-014 | `cause_in` に、`transport: connection_reset: io: Connection reset by peer (os error 104)`、語で始まらない transport の text 2 つ（`transport: the stream ended before its last line`、`transport: no scripted answer`）、`transport: ` の後が空の text、`transport: connection_reset`（語だけで `:` が無い text）、`status 503`、`body: no choices`、`aborted` を順に渡す | 一覧の語、一覧の外の transport、transport の後が空、語だけの transport、transport でない失敗 | 該当なし | 順に `Some("connection_reset")`、`Some("other")`、`Some("other")`、`Some("other")`、`Some("connection_reset")`、`None`、`None`、`None` | 陽性、陰性 | `a_failure_text_names_its_cause_only_when_it_is_a_transport_failure` |

**測らない物**：

- 実際の llama-server が無通信の接続を 5 秒で閉じる瞬間の競合：偽の server は FIN で閉じる形を作るだけで、cpp-httplib の時計を再現しない。1 件の失敗の原因は、R8z-1 の後の BR-3 の流し直しで決める（H166）。機械では当たらない。
- 本文を読む段（応答の先頭の後）の失敗：送り直さないので、test に載せない。

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
  - `写さない：Python の口（<Rust の在り処、または持たない機構>）`：5 つ目の種類。Rust が意図して持たない Python の口で、括弧の中に Rust の在り処（file、関数、受入 ID の行）か、持たない機構の理由を書く。見出しの語だけの行と、括弧の中が空の行は許さない（T-RP-19-011、T-RP-19-012）。
- 行の数は 386 で、Rust の名前を持つ行は 314、`写さない` が 72 である。72 は上の 4 種の 52 と、Python の口の 20 の和である。RP-19 の期待値は、名前を持つ 314 行が list に在ることと、`写さない` の 72 行が理由を持つことである。`-` の行は T-RP-19-006、T-RP-19-007、T-RP-19-011 から T-RP-19-013 が理由の有無を確かめる。
- Python の口の 20 行のうち 5 行（`test_ws_ingest.py` の server 側の handshake）は、固定の一覧 `rp19_not_yet_at_r11.tsv` に在る行を、この表で写さないと閉じた物である。固定の一覧は R11 の時点の未実装を sha256 と行の数で留めた物なので書き替えず、未実装の集合の期待値の側で引く。期待値は、固定の一覧の名前から、`rp19_ported/` の名前と、この表で Rust の名前が `-` の行の名前（case の名前から先頭の `test_` を除いた物）を引いた集合である。
- 置き場が `hayamimi-eval/src/ws.rs` の行は、`test_ws_ingest.py` の `/ingest` の手書きの client の codec（accept の値の式と、frame の組みと読み）の 8 行である。同じ file の server 側の handshake の 5 行（要求の読みと、応答の組み）は、`hayamimi` の `/ingest` を axum の `WebSocketUpgrade` が受けて Rust が自前で持たないので、`写さない：Python の口（…）` で閉じる（T-RP-19-023、T-RP-19-024）。

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
| `test_audio_capture.py` | `test_cli_rejects_loopback_on_non_windows_before_loading_models` | `-` | 写さない：Python の口（3.15 は要る物が無いことで起動を止めないので、loopback の無い OS では取り込みの落とし（`capture.fallback.single` か `none`、RP-29 の R5 の行）で動き続け、拒む口を持たない） |
| `test_audio_capture.py` | `test_microphone_device_selection_uses_input_only_case_insensitive_match` | `microphone_device_selection_uses_input_only_case_insensitive_match` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_loopback_rejects_corrupt_channel_or_window_shapes` | `-` | 写さない：Python の口（Rust の loopback は `wasapi` が返す型つきの format（channel の数と frame の数）から窓を組むので（`capture/windows.rs`、`capture/convert.rs`）、形の壊れた配列を受ける口が無い） |
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
| `test_audio_capture.py` | `test_mix_stall_drops_both_sides_symmetrically_and_keeps_time_alignment` | `-` | 写さない：Python の口（Rust の mix はマイクの窓を時計にして同じ時刻の speaker の窓を足し（`capture/combine.rs`）、speaker の側が詰まってもマイクの窓を捨てないので、両側を捨てる形を持たない） |
| `test_audio_capture.py` | `test_mix_waits_for_a_closer_packet_instead_of_consuming_older_candidate` | `mix_waits_for_a_closer_packet_instead_of_consuming_older_candidate` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_mix_does_not_retry_failed_soundcard_import_on_capture_thread` | `-` | 写さない：Python の実行時の package の import に固有（`soundcard` の import の再試行）。取り込みの装置が開けない時に止めずに落とす Rust の側の振る舞いは、RP-29 の R5 の行（`capture.fallback.none`）が持つ |
| `test_audio_capture.py` | `test_mix_speaker_failure_falls_back_once_to_mic_only` | `mix_speaker_failure_falls_back_once_to_mic_only` | `hayamimi/src/capture.rs` |
| `test_audio_capture.py` | `test_mix_microphone_failure_is_fatal_and_stops_speaker` | `-` | 写さない：Python の口（3.15 はマイクが開けない時に在る音源だけで動かし、`capture/combine.rs` は speaker だけの窓をそのまま流すので、致命にする形を持たない） |
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
| `test_embed_runtime.py` | `test_eventhub_add_listener_receives_the_raw_dict_synchronously` | `-` | 写さない：Python の口（Rust の `Hub`（`server.rs`）は broadcast の channel で文字列の event を配り、同じ process の中で同期に呼ぶ listener の口を持たない。埋め込む側は `/events` の SSE を読む（3.8）） |
| `test_embed_runtime.py` | `test_eventhub_multiple_listeners_all_receive_the_event` | `eventhub_multiple_listeners_all_receive_the_event` | `hayamimi/src/events.rs` |
| `test_embed_runtime.py` | `test_eventhub_listener_exception_is_swallowed_and_reported_once` | `-` | 写さない：Python の口（同期の listener の口を持たないので（`eventhub_add_listener_receives_the_raw_dict_synchronously` の行）、listener が投げる例外も無い） |
| `test_embed_runtime.py` | `test_eventhub_a_second_broken_listener_gets_its_own_report` | `-` | 写さない：Python の口（同期の listener の口を持たないので、2 つ目の壊れた listener も無い。配る口は `server.rs` の `Hub` の broadcast の channel だけである） |
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
| `test_embed_runtime.py` | `test_set_punctuate_reenable_after_degrade_clears_punct_for_retry` | `-` | 写さない：Python の口（Rust は句読点の model を起動時に 1 回開き、開けなければ 3.15 の形で落として実行中に読み直さない。`Engine::set_punctuate` は on と off の切り替えだけで、再試行する状態を持たない） |
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
| `test_embed_runtime.py` | `test_control_queue_poll_sets_event_and_continues_past_a_raising_callable` | `-` | 写さない：Python の口（Rust の命令は closure ではなく data の `Command`（`control.rs`）で、投げる命令が無い。適用の失敗は `session.rs` の `apply` の `Result` で loop に返る） |
| `test_embed_runtime.py` | `test_control_queue_task_runs_on_the_thread_that_calls_poll_not_the_submitter` | `-` | 写さない：Python の口（命令が closure でないので、走る thread を問う物が無い。命令を decode の loop で適用することは `run_stream_applies_a_submitted_task_on_the_decode_thread_not_the_submitter`（R12b）が持つ） |
| `test_embed_runtime.py` | `test_run_stream_polls_control_once_per_chunk` | `-` | 写さない：Python の口（Rust の session の loop は窓ごとに届いた命令を全部適用する（`session.rs` の `apply_commands`）。1 窓に 1 回 poll する口は無い） |
| `test_embed_runtime.py` | `test_run_stream_without_control_is_unaffected` | `-` | 写さない：Python の口（Rust の `Session` は命令の受け口を常に持ち、受け口の無い形が無い） |
| `test_embed_runtime.py` | `test_run_stream_applies_a_submitted_task_on_the_decode_thread_not_the_submitter` | `run_stream_applies_a_submitted_task_on_the_decode_thread_not_the_submitter` | `hayamimi/src/stream.rs` |
| `test_embed_runtime.py` | `test_config_and_reset_404_without_controls_attached` | `-` | 写さない：Python の口（Rust の server は `/config` と `/reset` を session の命令の受け口へ常に結び（`routes.rs`）、受け口を付けない server の形が無い） |
| `test_embed_runtime.py` | `test_get_config_reports_current_values` | `get_config_reports_current_values` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_config_applies_a_subset_of_keys` | `post_config_applies_a_subset_of_keys` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_config_invalid_value_returns_400_with_message` | `post_config_invalid_value_returns_400_with_message` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_reset_calls_the_reset_function` | `post_reset_calls_the_reset_function` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_reset_pending_returns_202` | `post_reset_pending_returns_202` | `hayamimi/src/server.rs` |
| `test_embed_runtime.py` | `test_post_reset_without_reset_fn_returns_400` | `-` | 写さない：Python の口（`/reset` は session の命令の受け口へ常に結ぶので、reset の関数の無い形が無い。loop が止まった時は `routes.rs` の `reset` が `applied(false)` を返す） |
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
| `test_lifecycle.py` | `test_run_clip_leaves_no_engine_behind` | `-` | 写さない：Python の口（Rust の `Engine` は持ち主の scope を出た時に `Drop` で手放され、process 全体から引く global や回収待ちの参照を持たない） |
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
| `test_ws_ingest.py` | `test_compute_accept_key_matches_rfc_example` | `compute_accept_key_matches_rfc_example` | `hayamimi-eval/src/ws.rs` |
| `test_ws_ingest.py` | `test_parse_handshake_request_extracts_path_and_key` | `-` | 写さない：Python の口（`/ingest` の upgrade の要求は `hayamimi/src/ingest.rs` の `upgrade` で axum の `WebSocketUpgrade` が読み、path と `Sec-WebSocket-Key` を取り出す server 側の読みを Rust は自前で持たない。その key に server が返す status と accept は `routes_and_handshake_match_status_and_keys`（T-RP-12-010）が録りと照らす） |
| `test_ws_ingest.py` | `test_parse_handshake_request_rejects_non_websocket` | `-` | 写さない：Python の口（websocket の upgrade でない要求を拒むのは、`hayamimi/src/ingest.rs` の `upgrade` が受ける axum の `WebSocketUpgrade` の extractor で、Rust は拒む口を自前で持たない） |
| `test_ws_ingest.py` | `test_parse_handshake_request_rejects_missing_key` | `-` | 写さない：Python の口（`Sec-WebSocket-Key` の無い要求を拒むのも、`hayamimi/src/ingest.rs` の `upgrade` が受ける axum の `WebSocketUpgrade` の extractor で、Rust は要求を読む口を自前で持たない） |
| `test_ws_ingest.py` | `test_build_handshake_response_contains_correct_accept` | `-` | 写さない：Python の口（101 の応答と `Sec-WebSocket-Accept` は、`hayamimi/src/ingest.rs` の `upgrade` で axum の `WebSocketUpgrade` が組み、Rust は応答を組む口を自前で持たない。accept の値の式は `compute_accept_key_matches_rfc_example`（`hayamimi-eval/src/ws.rs` の `accept_key`）が持ち、server の答えは T-RP-12-010 が録りと照らす） |
| `test_ws_ingest.py` | `test_build_handshake_request_roundtrips_through_parse` | `-` | 写さない：Python の口（要求を組む client の半分は `hayamimi-eval/src/ws.rs` の `upgrade_request` に在るが、読み戻す server の半分は axum の `WebSocketUpgrade` で、往復させる自前の読みが無い。録った要求の key に server が正しい accept で答えることは T-RP-12-010 が持つ） |
| `test_ws_ingest.py` | `test_frame_roundtrip_unmasked_binary` | `frame_roundtrip_unmasked_binary` | `hayamimi-eval/src/ws.rs` |
| `test_ws_ingest.py` | `test_frame_roundtrip_masked_text` | `frame_roundtrip_masked_text` | `hayamimi-eval/src/ws.rs` |
| `test_ws_ingest.py` | `test_frame_masked_bytes_differ_from_plaintext_on_wire` | `frame_masked_bytes_differ_from_plaintext_on_wire` | `hayamimi-eval/src/ws.rs` |
| `test_ws_ingest.py` | `test_decode_frame_incomplete_returns_none` | `decode_frame_incomplete_returns_none` | `hayamimi-eval/src/ws.rs` |
| `test_ws_ingest.py` | `test_decode_frame_two_frames_in_one_buffer` | `decode_frame_two_frames_in_one_buffer` | `hayamimi-eval/src/ws.rs` |
| `test_ws_ingest.py` | `test_decode_frame_large_payload_uses_64bit_length` | `decode_frame_large_payload_uses_64bit_length` | `hayamimi-eval/src/ws.rs` |
| `test_ws_ingest.py` | `test_frame_close_and_ping_opcodes_roundtrip` | `frame_close_and_ping_opcodes_roundtrip` | `hayamimi-eval/src/ws.rs` |
| `test_ws_ingest.py` | `test_decode_pcm16_mono_roundtrip` | `decode_pcm16_mono_roundtrip` | `hayamimi/src/capture/convert.rs` |
| `test_ws_ingest.py` | `test_decode_pcm16_drops_trailing_odd_byte` | `decode_pcm16_drops_trailing_odd_byte` | `hayamimi/src/capture/convert.rs` |
| `test_ws_ingest.py` | `test_decode_pcm16_averages_stereo_to_mono` | `decode_pcm16_averages_stereo_to_mono` | `hayamimi/src/capture/convert.rs` |
| `test_ws_ingest.py` | `test_decode_pcm16_empty_input` | `decode_pcm16_empty_input` | `hayamimi/src/capture/convert.rs` |
| `test_ws_ingest.py` | `test_resample_linear_identity_when_rates_match` | `resample_linear_identity_when_rates_match` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_resample_linear_upsamples_to_expected_length` | `resample_linear_upsamples_to_expected_length` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_resample_linear_downsamples_to_expected_length` | `resample_linear_downsamples_to_expected_length` | `hayamimi-asr/src/audio.rs` |
| `test_ws_ingest.py` | `test_resample_linear_preserves_a_ramp_shape` | `resample_linear_preserves_a_ramp_shape` | `hayamimi-asr/src/audio.rs` |
