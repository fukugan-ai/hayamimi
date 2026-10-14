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

各受入 ID（RB-1〜RB-8、RP-1〜RP-29）の下の case 行（`T-RB-<n>-nnn`、`T-RP-<n>-nnn`）は、test-designer の席がこの節に書く。
設計の席は書かない。

### 4.1 case 行の読み方

case は `T-<ID>-nnn` で呼ぶ。
test 関数の名前は、2 の表と `rust_port.ja.md` の 7 の「受入 test の名前」の後ろに `_` と各行の「関数の末尾」を続けた物にする（例：`rb01_scoring_and_eval_packages_are_installed_all_five_versions_match`）。
実装の test の名前はこの名前と一致させる。

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
| T-RP-1-013 | 句読点の段を外した設定の `rp01.json`（口は実装の席が置く） | 実走の陰性 | 実走 | ja が Python より悪く、`check_scorecard` が `Err` で ja を名指し（specified、RP-1 の陰性対照） | 陰性 | `punctuation_off_run_is_worse_than_python_in_ja` |

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
| T-RP-5-007 | python の run | 陽性対照 | 時刻が取れる | 最初の `partial` の時刻が全発話で取れる（specified、RP-5 の陽性対照） | 陽性 | `python_run_yields_first_partial_times` |
| T-RP-5-008 | 速報の訳を有効にした run の fixture と、切った run の fixture | 数える道具の確かめ | 切った run は 0、有効の run は 1 以上 | 速報の訳の数の関数が 0 と 1 以上を返す（specified、RP-5 の陰性欄） | 陰性 | `partial_translation_counter_reads_zero_when_disabled` |
| T-RP-5-009 | `<ev> stream` の python と rust の run | 実走 | 実走 | `first_partial_delay` の p95 が python 以下、速報の訳が出た発話で時刻が `t_tr` より前（specified、RP-5） | 陽性 | `rust_partials_arrive_no_later_than_python` |

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
| T-RP-13-012 | `hayamimi-relay eval` を偽の llama-server に流す（`native/hayamimi-relay/tests/r8f_eval.rs`） | 1 文の確定の訳が E、T、空のどれか | 3 文 | E が V を通れば E、通らず T が通れば T、両方外れれば空行。行数は原文と同じ（derived、RP-13 の「V が両方の訳を止めた行は空の hyp」） | 陽性、境界 | `each_sentence_settles_on_the_edit_then_the_draft_then_an_empty_line` |
| T-RP-13-013 | 同じ偽の server に 1 文を流し、要求の body を読む | 依頼の形 | user message 1 つ | 「Translate the following Japanese text into Chinese.」で始まる message が 1 つ、`stop` が `["</s>"]`、`temperature` が 0、`cache_prompt` が真（specified、設計 3.6 の「訳の依頼の形」） | 陽性 | `a_request_names_the_language_in_one_user_message` |
| T-RP-13-014 | pairs に表に無い言語（`xx`）が混ざる | 訳せない言語 | 1 言語 | その言語を訳さず `translate.fallback.skipped`（warn、`HAYAMIMI_TRANSLATE_FALLBACK`、`reason` が `unsupported_target`）を 1 件出し、ほかの言語は続く（specified、設計 3.15 の止めない形） | 陰性 | `an_unlisted_language_is_left_out_with_an_event_and_the_others_go_on` |
| T-RP-13-015 | llama-server に届かない、または要求が 4xx | 測定を止める場合 | 1 行目、2 行目 | `Err` で言語と行番号を名指しし、hyp の file を書かない（derived。空の答えの採点が測定に見えるのを防ぐ） | 陰性 | `an_unreachable_translator_stops_the_run_without_writing`、`a_refused_request_names_the_language_and_the_line` |

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
| T-RP-17-004 | fixture：`build.json` の ORT の版が 1.31.0 で、`ort` が開いた ORT の版が退路の 1.28.2 | 版が違う | `Err` | `Err` で版を名指し（specified、RP-17 の「sherpa-onnx の build の ORT と同じ」。比べる相手は `build.json` の ORT の版） | 境界、陰性 | `ort_version_differing_from_build_json_is_rejected` |
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
| T-RP-22-008 | 実走：007 の run で測った最大より 1 MiB 低い値を上限に置いて、同じ run を起こす | 上限に届いた run（落ちた形） | 実走 | `hayamimi` が止まらず（終了 code 0、`final` が出る）、`gpu.fallback.cpu`（`reason` = `memory_budget`）が 1 件以上、`/events` の `fallback` が同じ数。`check_vram` は上限を超えた読みを名指しして `Err`（specified、RP-22 の陰性欄と 3.15 の memory の行） | 陰性 | `budget_reached_falls_back_instead_of_stopping` |
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
| T-RP-24-002 | fixture：ORT の版だけ 1.31.0 と退路の 1.28.2 | ORT の版 | `Err` | `Err` で ORT の版を名指し（specified、RP-24 の陰性欄） | 陰性 | `ort_version_difference_is_named` |
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
| T-RP-25-010 | 実走：RP-3 の Rust の run を読む | 実走 | 実走 | 全発話で 001 から 009 の判定を通る（specified、RP-25） | 陽性 | `translation_flicker_is_bounded_in_the_real_run` |

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
  - `写さない：<理由>`：3.2 が消すと決めた script の test、設計で無くなる部品（翻訳 worker、翻訳器の pool、文ごとの訳の分割）、外が更新する model の sha256 を gate にする case。写す先が無い。
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
| `test_audio_capture.py` | `test_windows_com_scope_balances_every_success_result` | `windows_com_scope_balances_every_success_result` | 置き場：`native/hayamimi/src/capture.rs`（COM の初期化と後始末は `hayamimi` が自前の scope で持つ、設計 3.7）。名前は test-designer の席が入れる |
| `test_audio_capture.py` | `test_windows_com_scope_uses_existing_changed_mode_apartment` | `windows_com_scope_uses_existing_changed_mode_apartment` | 置き場：同上。名前は test-designer の席が入れる |
| `test_audio_capture.py` | `test_windows_com_scope_rejects_other_hresult_failures` | `windows_com_scope_rejects_other_hresult_failures` | 置き場：同上。名前は test-designer の席が入れる |
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
| `test_audio_capture.py` | `test_mix_does_not_retry_failed_soundcard_import_on_capture_thread` | `-` | 写さない：Python の `soundcard` package の import の再試行に固有で、Rust には import が無い |
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
| `test_embed_runtime.py` | `test_translation_worker_publishes_on_hub_with_no_http_server` | `-` | 写さない：Python の in-process の翻訳 worker はこの設計で無くなり、訳は `hayamimi-relay` が出す（設計 3.6、RP-19）。relay の側へ写すかを設計が決める |
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
| `test_units.py` | `test_digits_pass_when_source_has_no_ascii_digits` | `digits_pass_when_source_has_no_ascii_digits` | 置き場：`native/hayamimi-relay` の照合 V の test（会話字幕の CS-4a）。数字の守りは V が持つ。名前は test-designer の席が入れる |
| `test_units.py` | `test_digits_pass_on_exact_and_superset` | `digits_pass_on_exact_and_superset` | 置き場：同上。名前は test-designer の席が入れる |
| `test_units.py` | `test_digits_fail_on_mangled_number` | `digits_fail_on_mangled_number` | 置き場：同上。名前は test-designer の席が入れる |
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
