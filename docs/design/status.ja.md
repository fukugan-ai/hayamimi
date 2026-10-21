# hayamimi の実装の状態

正本は日本語のこの文書で、英語版は [status.md](status.md) である。
意味が食い違ったときは、この文書を先に直してから英語版を直す。

この文書は、完成までに残っている物と、slice ごとの状態を書く。
冒頭の表が残っている物で、その後ろの表が slice ごとの状態、merge の commit、検証の出所、受入の通過、残る穴である。
slice の中身と通す受入は `rust_port.ja.md` の 8 に、要件は [requirements.ja.md](requirements.ja.md) に在る。
commit の番号は `git log --oneline` で確かめた値で、確かめた時の main の先頭は `09f91c1`（2026-10-21 の `feat/r13-5b` を `--no-ff` で入れた merge commit。1 つ前は `a74d6e8`（`feat/r13-2` と `design/config-values` の後に status と README を直した commit）で、その前の merge は `f951ef0`（`design/config-values`）と `abde91f`（`feat/r13-2`）。merge の後の木で test を打った。この文書を足した commit はその 1 つ上である）である。

## 完成までに残っている物

完成の条件は、実行時の経路から Python が無くなって消えることと、主指標 `L`（発話の終わりから訳の字幕が出るまでの遅れ）が合否の線（RP-67、p95 1600 ms 以下）を満たすことである。p95 1200 ms 以下と 1000 ms 以下は完成の後の目標である。
前者は R11 で満たした（Python の 78 本は消え、消す前の版は tag `archive/python-before-r11` にある）。
次の表は、その条件に届いていない物を、持ち主と止めている物つきで並べる。
「持ち主」は、その物を動かす席である。
記録に持ち主が無い物は「記録に無い」と書く。

| 残っている物 | 今の状態 | 持ち主 | 止めている物 | 根拠 |
|---|---|---|---|---|
| RP-32：Python と Rust を同じ負荷で交互に 3 対流す（E2E-1 の正式な判定） | 速度は、行ごとの線で 19 発話中 18 発話が Python より遅くない（必要は 17、余裕は 1）。300 ms の線では 17 / 19。この 2 つは `rp32` の `--ignored` の出力である。`L` の p50 は 885 ms、p95 は 1501 ms（E の形を変える前の run は 865 ms、1461 ms）。Python の基準は 2026-10-15 の `py1`〜`py3` に固定し、流し直さない。品質は、どちらも Rust のほうが Python より悪い。`final` が 2 つ以上の発話の割合は Rust 22.61 %、Python 8.70 %。CER は Rust 13.15 %、Python 12.95 %。この 2 つの値は E の形を変えても動かなかった。E の段の行のうち指示文の復唱に当たる行は、1 run あたり 1 件から 0 件になった。品質と復唱の数は E2E の席の申告で、この文書は打ち直していない。今回の run の記録は E2E の席の cache（repo の外）にあり、`docs/results/rp32/` の `rs1`〜`rs3` は E の形を変える前の run である | root-cause の席、実装の席 | `L` の p95 が 1461 ms から 1501 ms に悪くなったのは、3 run が前の 15 分平均の load 2.95〜3.14 の下で流れ、認識の CPU の段が一様に約 15 % 遅かったためである（root-cause の席の申告。前の 1 分の load は 1.77〜1.89 で今の門を通った。RP-83 の門がこの形の run を止める）。E の形の最初の依頼が遅いこと（`rs1` の最初の E が 1326 ms、実装の席）。発話 22 が `incomplete` のまま 3.3 s 持たれる原因が未特定であること（root-cause の席） | `83830af`、`a7f4db6`、`0900a32`、`e745bd3`、`docs/results/rp32/` |
| RP-33：T が V に落ちた行の最初の訳の段ごとの列 | 可（2026-10-16 の Rust の 3 対）。数は前の席の申告で、この文書は打ち直していない。記録は `docs/results/rp32/` | なし | なし | `fccce49`、`3ba5ef2`、`docs/results/rp32/` |
| RP-69：速報の訳を `final` で使い回す | 評議 GO・case 行を書いている | 評議の席 | 3.18 の評議 | `rust_port.ja.md` の 3.18、6 の RP-69 |
| RP-70：末尾の `。` を足しただけの文の訳を使い回してよいことの品質の確認 | 評議 GO・case 行を書いている | 評議の席 | 3.18 の評議 | `rust_port.ja.md` の 3.18、6 の RP-70 |
| RP-71：推測デコードの llama-server が速く、同じ訳を出すこと | 記録を読む test（`native/hayamimi-relay/tests/rp71.rs`）と、推測デコードの腕 S3 と下書きなしの N0 の計測の記録（`docs/results/r8w/` の `s3_*` と `n0_*`）は main に入った（`c7decfb`、`7522f58` で merge）。訳の試験用の llama-server の unit（`hayamimi-llama-pair.service`、127.0.0.1:18083）も入った（`38e54e7`、`0f79478` で merge）。残るのは、速さと同じ訳の判定と、E2E の run（計画の L6）である | 原因分析の席（速さの数）、3.18 の評議の席（判定） | 訳の速さの数は、原因分析の席が数えている。数が出るまで RP-71 の通過を名乗らない | `7522f58`、`0f79478`、`c7decfb`、`rust_port.ja.md` の 3.18、6 の RP-71 |
| RP-72：交互の run で port が移った run を無効にする | 評議 GO・case 行を書いている | 評議の席 | 3.18 の評議 | `rust_port.ja.md` の 3.18、6 の RP-72 |
| RP-66：発話の終わりの待ち `d` を縮める 2 段の区切り（R8v） | 手番 1 の流し直しで通す線から外れた。評議 PAUSE。合否を文の途中の切断の率に替えた。T の行は書き直した（T-RP-66-040〜063）。手番 1b（仕分けの probe）は E2E の窓が閉じてから。設計と、停止条件 2 に当たった後の設計の直し（RP-68、H75、H76）は main に入った。実装は未着手 | R8v の席 | 手番 1b（仕分け）の結果を、文の途中の切断の率で判定すること。held-out を 1 回流すこと。その後に手番 2 で RP-68 の test を通してから 2 本目の detector を作る | 設計 `27efb3c`、直し `4ca0f86`（`5522942` で merge）、合否の線の書き直し `3fb8875`、T の書き直し `bcf41e0`、D の 2 文 `2705ffe`、probe `f925f8c`（branch `feat/r8v-probe`、main に無い） |
| RP-67：`L` の p95 を 1600 ms 以下にする（ja と en の両方。1200 ms と 1000 ms は目標で、合否に置かない） | 判定の test `rp67.rs` は入った（19 本。実走の 1 本だけ `#[ignore]`）。実走の行（T-RP-67-015）は `docs/results/rp67/` の記録を要する。窓 E3'' の組（記録は `docs/results/r8x/` の `e3q*`）で通った。最後の判定は窓 E4 である。1200 ms の目標は、en が p95 1089.1 ms で内側、ja が 1484.8 ms と 1474.1 ms で外側にある。3.19 の組は未実装。今の main の記録（`docs/results/e2e/`）では、ja の `L` の p95 が 1871.7 ms（n 93）で 1600 ms の線の外、en の p95 が 1303.2 ms（n 70）で線の内側にある（T-RP-67-014 が読む）。1200 ms 未達、今の組の見積りは p95 約 1473 ms（推測）。発話の終わりの起点（`t_end`、manifest の `end_sample`）は今のまま置く。強制の位置合わせは作らない。組は `rust_port.ja.md` の 3.19（R8x）で、方針 B（H130）と C と D（H132）は外した | ja は R8x の席、en は R7 の席 | 今の build（`83830af`、RP-32 の 3 run）で ja の `L` の p95 は 1501 ms、1600 ms を超える標本 3、許す数 14（下の「主指標 `L` の内訳」）。今の組（E の初回、認識器の初回、鍵の広げ）の見積りは p95 1473 ms、1600 ms を超える標本 1。線は窓 E3' の後に動かさない。窓 E3'' の門の内側の run で通った（en は門の前の run `e1pre` で p95 1318 ms だった）。次は窓 E4 | `27efb3c`、`22b213a`、`rust_port.ja.md` の 3.19。窓 E3'' の rp67 の実走（PL-3）は ja が p95 1484.8 ms と 1474.1 ms、en が 1089.1 ms で、1600 ms を超えた標本は 0、ja を `e3q1` にした打ちも `e3q2` にした打ちも PASS。1200 ms を超えた標本は ja が 16 と 16、en が 2。窓 E4 の記録は残る。 |
| 窓 E1・E3' で落ちた受入（rp84、rp85 (4)、en の run の門 RP-83） | build `4deb57c` の窓 E1・E3'（ja 2 本、en 1 本）の記録が main に入った。RP-85 (3) の probe は通った。rp84 は落ちた（発話 1 が 591.8 ms と 659.0 ms。ほかの 97 発話の p95 は 191.3 ms と 229.8 ms）。rp85 (4) は落ちた（発話 7 の訳の段が p95 418.5 ms、線は 182 ms）。rp67 は、en の run の最後の load5 が 3.10（線は 2.50）の 1 点で落ちた。`L` の p95 は ja が 1456.4 ms と 1506.2 ms、en が 1140.8 ms で、1600 ms を超えた標本は 0。en の値は RP-83 で落ちた run の値で、窓 E3' の時点では RP-67 の合格は確定していなかった。数は窓の席の申告で、この文書は打ち直していない | 済み | 無し（実機は窓 E3'' で確かめた） | `0d531f3`、`docs/results/r8x/`。R8x-7（句読点の温め）が main に入った（offline の test。実機は窓 E3''）。R8x-8（RP-85 (4) の数え方）が main に入った（offline の test。実機は窓 E3''）。R8x-9（RP-83 の run 自身の分）が main に入り、3 つの slice が揃った（offline の test。実機は窓 E3''）。窓 E3''（build `da4ea82`、ja 2 本 `e3q1`・`e3q2`、en 1 本 `e3q1`）で 3 本とも通った。rp84 と rp85 (4) は ok（鍵が合う標本の訳の段の最大は 124.3 ms と 76.8 ms、線は 182 ms）。RP-83 の門は en が `load5` 2.52 から run 自身の分 1.57 を引いて 0.95（線は 2.50）、ja の 2 本目は `load5` 2.54 から 1.34（`own5` 1.345059 の切り捨て）を引いて 1.20。sherpa は CUDA 版の lib（`~/.local/share/hayamimi/build/cuda1343/linux/install/lib`）。数は窓の席の申告で、この文書は打ち直していない。 |
| RP-73：RP-67 の合否が、起点の置き方の誤りで決まっていないこと | 設計済みで未実装（評議 GO）。起点が確かな発話だけで数えても `L` の合否が変わらないことを test で確かめる | `L` を縮める仕事（H93、VAD、訳の段）の後に出す実装の席 | RP-67 が初めて通ったと名乗る前に要る。それより前に、`L` を縮める仕事から cargo の席と load の窓を取らない | `22b213a`、`rust_port.ja.md` の 3.1 と 6 の RP-73 |
| RP-74：認識の encoder だけを GPU に置く形（方針 D）の見積り | 設計済みで未実装（評議 GO）。認識の置き場は、RP-78 が通るまで CPU（int8）のまま | E2E の後の窓の席 | E2E の窓が閉じること。RP-74 の後に RP-78（split の形で `L_asr` が縮み、訳が遅くならない）が通ること | `9f88e1e`、`rust_port.ja.md` の 3.5 と 6 の RP-74、RP-78 |
| RP-1 の ko、T-RP-1-013 の実記録、RP-10 の `lang` の条件。RP-22 と言語判定の結線は通過 | R3b と R3c は検証済みで merge 済み。RP-22 は peak 8936 MiB（上限 14000）で通過。RP-1 は ja、en、zh、yue が Python と同じ値で、ko だけが配る形の build で 0.087（Python は 0.081） | ko は評議が R11 の入口の上限（RP-36）で受けた。T-RP-1-013 は E2E の席。RP-10 は Windows の build を持つ席 | ko は ORT の build の違いで、閾値は緩めない（H35）。H55 は Windows の build が無いと書く | `0358773`、`b8fd07e`、`docs/results/r3/rp22_run.json` |
| RP-44：新しい held-out の組（RP-35 の最終の測りに使う） | 組はできた（`testdata/heldout/set2/`、git の外。en、zh、ko、es 各 35 行） | R12y の席 | 組を使う入口（`hayamimi-relay eval` の `--final`、`--heldout`、`--direction`）と、同じ組を 2 回使う拒み（CS-30）が未実装 | `rust_port.ja.md` の 8 の R12y、README |
| RP-9、RP-19：Python を消す（R11） | R11 は入った。Python の 78 本と `requirements*.txt` は消え、CI は cargo で動く。RP-19 の未実装の行は `rp19.rs` が毎回数え、数は CI の専用の step が出す | R13 の席 | R13 の各 slice が写した名前を `rp19_ported/` の file に足し、固定の一覧が空になるまで `rp19_python_cases_are_ported` は落ちる。file の名前は持ち主の R12 の札で、札を 1 つの slice が持つ時は `<札>.txt`（`R12e.txt`）、複数の slice が分けて持つ時は `<札>.<R13 の slice>.txt`（`R12b.R13-1.txt`）。後者は `rp19.rs` が札を最初の `.` より前で読むので置ける（`rust_port.ja.md` の H134） | `rp19.rs`、`rp09.rs`、`rp80.rs`、`rp81.rs`、`docs/results/r11/ci_probe.md` |
| 重ね窓（`hayamimi-overlay`、透明で最前面の窓） | crate は main に入った（OV-S2、OV-T）。窓を開く受入 OV-V の検証は「不可」を返した。不可の中身は CS-11、OV-2、OV-5 と、H28 の `missing_glyphs`（字形の欠け）である。直しの席が動いている。CS-10〜CS-12 は未通過 | 重ね窓の席、直しの席 | OV-V の不可の 4 つを直して、検証の席に打ち直させること。`rust_port.ja.md` の 8 に slice の行が無い。設計は会話字幕の設計 3.12、受入は CS-10、CS-11、CS-12 | `git ls-tree origin/main native/` |
| Zoom と Teams への送信の実機（CS-17、CS-18） | R8g は偽の受け手で検証済み。実機は打っていない | 記録に無い | Zoom の host の token と Teams の主催者の link が要る。手元にあるかは NOT VERIFIED | `a496066`、`state.json` の `blocked_on` |
| 動画ファイルから SRT への実機（CS-14、CS-15） | R8g は検証済み。実機は日本語 9 秒の動画 1 本を CPU で通しただけ | 記録に無い | 英語の行、翻訳器、GPU、長い動画は通していない。MPC-HC が `<動画名>.ja.srt` を自動で読むかは NOT VERIFIED（会話字幕の設計 9 の H3、H18） | `a496066` |
| OBS の字幕頁を OBS の中で測る（CS-16） | 未実施 | 記録に無い | 利用者の OBS が仮想カメラを使っていないときに、別の portable の OBS でだけ測る（会話字幕の設計 9 の H11） | 会話字幕の設計 9 |
| マイクの実機（E2E-1） | 2026-10-15 15:43 に RDP のマイクで process A を起こし、起動までを機械で確かめた。記録は `docs/guide/run_hayamimi.ja.md` に入った。`capture.fallback.none` は 0 件、`asr.final.published` は 8 件 | 利用者の発話 | 人が日本語を話した結果は機械では確かめられない | `2d18f1d`（`5ad5da1` で merge） |
| 対面のモード（1 本のマイクで日英の双方向） | 設計済み（評議 GO）。実装は R8v の merge の後の IP-S3 から | 設計の席 | R8v の merge。設計は `conversation_subtitles.ja.md` の 10 節、受入は IP-1〜IP-7 | `1ba9af7`（`docs/in-person`） |

YouTube Live の字幕取り込みへの送信は作らない（会話字幕の設計 1 と 4）。
配信では OBS の字幕頁を映像に焼き込む経路を使う。

## 完成までの計画

### 順位

1. Python を消す（R11 と RP-19、下の P0〜P5）。
2. 重ね窓（`hayamimi-overlay`、下の O1、O2）。
3. `L` の p95 を縮める仕事（下の L1〜L11）。

席と cargo の並列と main の lock は、この順位で先の物へ渡す。
3 位の仕事は、1 位と 2 位が使わない資源（GPU と E2E の窓、空いた CPU）でだけ進める。
P と O の slice の数と中身は、Python を消す設計（branch `design/python-removal`、`rust_port.ja.md` の 3.2 と 8）と重ね窓の設計（branch `design/overlay`、`conversation_subtitles.ja.md`）が決める。
この節の P と O の行は、2 つの設計が入る前の `rust_port.ja.md` の 8 と会話字幕の設計 3.12 の記録から数えた物で、印【設計の席の返答で直す】を付けた行は、2 つの設計の席の返答で数を直す。

### 結論

Python は 2026-10-20 に消えた（main `9bd6ec3` で `git ls-tree -r --name-only` の `.py` が 0 本）。重ね窓が通るのは 2026-10-24 の見込みである【設計の席の返答で直す】。
RP-67 の合否の線は p95 1600 ms である（評議の判定。`rust_port.ja.md` の 3.19 と H132）。1200 ms と 1000 ms は目標として残し、合否に置かない。
完成の表は「1200 ms 未達、現在 p95 約 1473 ms」と書く。1600 ms は退いた線で、悪くなることを止める線であり、速さの目標ではない。
完成は 2026-10-26 の夕方〜夜の見込みである。上限「2026-10-26 の夜〜2026-10-27 の未明、悪い側 2026-10-27」は保つ。この日付は、線が 1600 ms である間だけ成り立つ。
どちらの日付も、実装の merge が 1 日あたり 8.3 本の速さで毎日続くことを前提にした推測である。
受付は 2026-10-23 を目標に回す。10-23 は、休まず回す時の速さ 1 日 23 本の見積り（下の PL-H2）による推測である。
1000 ms の目標に届かせるには、発話の終わりの待ち `d` を作り直す設計と実装（下の L8）が要る。1200 ms の目標に届かせるには、無音で閉じた 39 標本（発話 16、22、12、11）を縮める手が要り、完成の後の別の設計に置く。
RP-67 は ja（E2E-1）と en（E2E-2）の両方を 1600 ms で見る。en は門の内側の run の値（下の L9 の流し直し）が出るまで、通過を名乗らない。
完成と呼ぶのは次の 3 つが揃った時である。

1. R11 が入り、実行時の経路から Python が消えている（RP-9、RP-19）。満たした。RP-19 の写しは R13 が続ける。
2. 清書（Refiner）まで入った build で、RP-67 が ja と en の両方で通り、RP-73 も通る。
3. 重ね窓（`hayamimi-overlay`）が在り、CS-10、CS-11、CS-12 が通る。

3 つ目は、この節より上の完成の条件の文に無い。
入れたのは、state.json の `goal` と要件の FR-9 が重ね窓を持つからで、外すかは評議が決める（残る穴の PL-H1）。

### 数え方と数

| 数えた物 | 範囲と式 | 数 | 対照 |
|---|---|---|---|
| 残っている物の行 | この文書の「完成までに残っている物」の表の行（見出しと区切りを除く。`awk` で節を切り `grep -vc`） | 19 行。うち RP-33 は「可」なので開いている行は 18 | 同じ式で「slice ごとの状態」の表が 32 行 |
| Python の file | `git ls-files '*.py'` | 78 本（`scripts/` 56、`tests/` 20、`demo/` 2）と `requirements*.txt` 2 本 | 3.2 の 78 本と一致 |
| RP-19 の未実装 | `rust_port.ja.md` の 8 の「RP-19 の未実装 187 行の仕分けと slice」 | 187 行（(a) 111、(c) 57、写さない 15、材料が無い 4） | `native/hayamimi-eval/tests/` に `rp09.rs`、`rp73.rs` は 0 本（`ls` の rc 2）。`rp19.rs` と `rp67.rs` は在る。同じ dir の `rp` で始まる `.rs` は 27 本（`ls rp*.rs | wc -l`） |
| R12 の branch | `git for-each-ref` で名前に `r12` を含む branch | 0 本 | — |
| 重ね窓の crate | `ls native/` | 無い（crate は 7 つ） | — |
| 実装の merge の速さ | main の first-parent の merge のうち `feat/`、`fix/`、`test/`、`probe/` の物（2026-10-13〜2026-10-20 10:25） | 54 本。日ごとに 10-14 が 23、10-15 が 23、10-16〜10-18 が 0、10-19 が 6、10-20 が 2。6.5 日で割って 1 日 8.3 本 | — |
| 1 本の長さ | 同じ 54 本の、最初の commit から merge までの分 | p50 51 分、p75 165 分、最大 6117 分 | 8 の slice の見積り（25〜60 分）と p50 が合う |

量の見積りは、8 の表に分の見積りが在る slice はその値を、無い物は 1 本 51 分（上の p50）を当てた。
8 に行が無く、過去に似た slice も無い物は「推測」と書く。

### 主指標 `L` の内訳

`L` の上位の標本が、どの段で遅れているかを、RP-32 の Rust の 3 run（`83830af` の build、git の外の E2E の記録 `rs1`〜`rs3`）から `jq` で数えた。
数え方は RP-67 と同じで、有効は `d ≥ 120 ms`、p は nearest-rank、`d = (t_final − t_end)/1000 − final_latency_ms`、復号は `final_latency_ms`、訳は `(t_tr − t_final)/1000` である。
陽性対照として、`L` が n 291、p50 885 ms、p95 1501 ms となり、RP-32 の test の出力と一致した。

| 段 | p50 | p95 |
|---|---|---|
| `L` | 885 | 1501 |
| 待ち `d` | 662 | 1013 |
| 復号 | 123 | 217 |
| 訳 | 128 | 385 |

訳の段の p95 は 385 ms で、H69 が根拠にした 1034 ms（`e2e1` の n 93、前の build）ではない。
`d` だけで p95 が 1013 ms あり、復号と訳を 0 にしても `L` の p95 は 1000 ms を下回らない。
1000 ms を超える標本は 94、1200 ms を超える標本は 64 で、p95 を通すために超えてよい数は 14 である。
1200 ms を超える 64 標本のうち 45 は発話 7、16、22 の 15 回ずつで、どれも `d` の平均が 920〜1040 ms である。
発話 16 と 22 は無音（`silence`）で確定し、発話 7 は早い終わり（`early`）で確定している（`asr.endpoint.decided` の `rule` を `final` の `id` で結んだ数え。`rust_port.ja.md` の 3.19）。
残りは発話 12 が 6、11 が 4、1 と 2 と 21 が 3 ずつで、発話 1 は各 run の最初の発話で復号が平均 826 ms ある。

### 1000 ms の線との差 501 ms の埋め方

数は、上の 291 標本の段の値を置き換えて p95 を引き直した値である。
段ごとの縮みは足し算では効かないので、組み合わせは組み合わせた形で引き直した。
どれも記録の上の計算で、実際の run の値ではない（推測）。

| 仕事 | 置き換え方 | `L` の p95 | 縮み |
|---|---|---|---|
| 今 | — | 1501 | — |
| E の初回の依頼（`feat/e-warmup`） | 各 run の最初の発話 1 標本の復号と訳を p50 にする | 1481 | −20 |
| 影の印（`feat/held-tail-shadow`）、VAD の trace（`feat/vad-trace`） | 測る物で、判定を変えない | 1501 | 0 |
| 方針 B（RP-79、`design/vad-end`） | 発話 16 と 22 の `d` を B の検出の遅れ（再生の 591 ms と 476 ms に E2E との差 25 ms を足す）で頭打ちにする | 1432 | −69 |
| 方針 B と encoder を GPU に（RP-74、RP-78） | 上に、復号を一律 −32 ms（3.5 の見積り） | 1400 | −101 |
| 方針 B、RP-78、推測デコード（RP-71）で訳が半分 | 訳の速さは測っていない（ASSUMPTION） | 1229 | −272 |
| 同じで訳が 0 | 訳の縮みの上限 | 1064 | −437 |
| 無音を 0.12 秒にする（3.17 の案 b） | `d` を一律 −192 ms | 1321 | −180 |
| B を単独で使う（3.17 の案 c） | `d` を一律 −576 ms | 937 | −564 |
| B を単独、RP-78、訳が半分 | — | 724 | −777 |

方針 B の再生の p95 −127 ms は検出の遅れの縮みで、`L` の p95 には約 −69 ms しか出ない。
p95 を決める発話 7 が、方針 B でも無音で確定したまま残るからである。
今の設計に在る仕事を全部入れても、ja の `L` の p95 は 1064〜1229 ms にとどまり、1000 ms に届かない。
届く形は、`d` を全標本でおよそ 310 ms 以上（訳が半分の時）縮める物だけで、記録に在る候補は案 c（B を単独で使う）である。
案 c は割れが ja で 4 から 13 に増えるので、文の途中の切断の率（RP-66）を守る形で `d` を作り直す設計が要る（下の L8）。
en は今の build で `L` を測っていない（E2E-2 の 1303 ms は前の build で、上位に初回の読み込みの 4681 ms を含む）。

### 1600 ms の線と、1200 ms の目標との差

今の組は、E の初回、認識器の初回の復号（RP-84）、使い回しの鍵の広げ（RP-85）の 3 つで、方針 B と C と D は外した（`rust_port.ja.md` の H130 と H132）。
数は上の 291 標本の段の値を置き換えて引き直した値である（推測）。

| 組 | `L` の p95 | 1200 ms を超える標本 | 1600 ms を超える標本 |
|---|---|---|---|
| 今 | 1501 | 64 | 3 |
| 今の組（B、C、D なし） | 1473 | 58 | 1（発話 2 の 1830 ms） |
| 今の組に C を足した時 | 1473 | 48 | 1 |

許す数は 14 で、今の組は 1600 ms の線を通り、1200 ms の目標には 44 標本届かない。
1600 ms の余裕は、全標本に一律 +120 ms で線を超える標本 12（通る）、1550 ms の線に +80 ms で 16、1500 ms の線に +40 ms で 17（どちらも落ちる）である。
下の表は、方針 B を入れた時の値で、今の組の見込みではない。

| 組 | `L` の p95 | 1200 ms を超える標本 |
|---|---|---|
| 今 | 1501 | 64 |
| 方針 B（RP-79。発話 16、22 と、名詞で終わる発話 12 の 3 標本） | 1433 | 31 |
| ＋ E の初回（済み）と認識器の初回の復号（RP-84、下の L5） | 1402 | 28 |
| ＋ 使い回しの鍵の広げ（RP-85。数字の並びと `。` `、` だけが違う速報の T の答えを使う） | 1287 | 25 |
| ＋ 早い終わりの 3 本目の detector C（RP-86。終わりの線 0.65） | 1215 | 15 |
| ＋ 持った候補の短い無音の確定 D（RP-86。接続の形と助詞で終わる文には使わない） | 1162 | 12 |

発話 7 の 15 標本は早い終わりで確定しているので、無音 0.12 秒（3.17 の案 b）も L8 の小さい形も発話 7 に効かない。
`d` を全標本で一律に縮めた置き換え（案 b で 1218 ms、RP-78 と合わせて 1186 ms）は、効かない標本にまで縮みを当てた値で、使わない。
推測デコード（RP-71 の S3）は訳の p50 を 29 % 遅くしたので、完成の道に置かない。
RP-78（L7）も完成の道に置かず、1200 ms と 1000 ms の目標の道に残す。
R8x-5 の offline の門は落ち（L15）、C と D は両方外した。線は窓 E3' の run（下の PL-3）の前に 1 回だけ置いた物で、E3' の後に動かさない。

### 残っている物

「直接」は完成の 3 つの条件のどれかに効く物、「間接」は効かない物である。

| ID | 物 | 直接か | 依存 | 資源 | 量 | 出所 |
|---|---|---|---|---|---|---|
| P0 | R12a1（`rp19.rs`、`rp19_not_yet_at_r11.tsv`）。R11c の前に merge 済み | 直接（RP-19） | なし。他の R12 の門 | cargo | 55 分 | 8 の「作る slice」 |
| P1 | R12b〜R12g（(a) 111 行、6 slice）【設計の席の返答で直す】 | 直接 | P0 | cargo、`session.rs` を触る R12b は P2 の R12h、R12i と順 | 6 × 51 分（推測。8 の見積りは build と直しの 180 分だけ） | 8 の「13:55 までに入る組」 |
| P2 | R12h、i、v、q、r、s1、s2、t1、t2、t3、u1、u2、x（(c) 13 slice）【設計の席の返答で直す】 | 直接 | P0。s2 は s1、t2 は t1、t3 は t2、u2 は u1 の後 | cargo。R12r は Windows の build | 535 分 | 8 の「作る slice」 |
| P3 | 清書（RP-61）。R12w0 は済み。R13-13 が部品（R12w1〜R12w3 を 1 席）、R13-20 が live への結線と `--no-refine`、R13-19 が `rp61.rs` を作り、RP-61 の run は窓 E4 で取る | 直接 | R13-13 → R13-20 → R13-19 → RP-73 → 窓 E4 | cargo。RP-61 の run は窓 E4（E2E の窓と GPU、Windows の実機） | 250 分（R13-13 の 165、R13-20 の 40、R13-19 の 45。推測）。run は窓 E4 の約 2 時間に入る | 8 の「清書を live に足す形」と R11 の後の組 |
| P4 | R12a2（`rp09.rs`） | 直接（RP-9） | P0 | cargo | 40 分 | 8 の「作る slice」 |
| P5 | R11（78 本と `requirements*.txt` を消し、CI を cargo に、Dart の comment を書き替える）。R11c で入った | 直接 | 済み | main の lock | 済み | 8 の R11、3.2 |
| P6 | R13-11b：4 クラスの句読点を `Engine::open` へ結ぶ。R13-11 が入っても `--punct-model` は `hayamimi` の引数に無く、`punct::select` を `Engine::open` が呼ばないので、4 クラスの句読点は live の run から選べない。結ぶまで RP-59 は通ったと名乗らない | 間接（既定は BERT のまま） | R13-11、R13-20 | cargo | 40 分（推測） | 8 の R11 の後の組 |
| L1 | 3 本の merge（`feat/e-warmup`、`feat/held-tail-shadow`、`feat/vad-trace`）。済み（下の「slice ごとの状態」に merge 済みと在り、3 本とも `git ls-remote origin` に無い） | 直接（RP-67 の材料） | 済み | なし | 済み | 下の「slice ごとの状態」 |
| L2 | H100 の offline の門（方針 B の手順 2） | 済み。門は落ちた（`origin/feat/l2-shadow-gate` の `d024bbe`） | — | — | — | `rust_port.ja.md` の H130 |
| L3 | `--early-complete nouns` と `asr.endpoint.widened` | 止める（B を組から外した） | — | — | 51 分が外れる | 同上 |
| L4 | RP-79 の E2E（窓 E2） | 止める（同上） | — | — | 約 3 時間が外れる | 同上 |
| L5 | 認識器の初回の復号を `/ingest` の前に済ませる（R8x-3、RP-84）。済み（`fb08a2d`） | 直接 | なし（設計は 3.19） | cargo | 51 分 | `rust_port.ja.md` の 3.19 |
| L6 | RP-71 の推測デコードの probe と E2E | 間接（完成の道に置かない。S3 は訳の p50 を 29 % 遅くした） | 3.18 の評議 | GPU。18081 と 18083 の llama-server は同時に載らない | 2 slice と E2E 1 時間（推測） | 3.18 |
| L7 | RP-74 の見積りと RP-78 | 間接（1200 ms と 1000 ms の目標の道） | E2E の窓が閉じた後 | GPU、E2E の窓 | 3 slice（推測） | 3.5、RP-74、RP-78 |
| L8 | 文の途中の切断の率を守って `d` を全標本で 310 ms 以上縮める設計と実装 | 間接（1000 ms の目標の道） | 設計の席と評議、L4 の結果 | 設計の席、cargo、RP-66 の E2E | 設計 1、評議 1、実装 3、RP-66 の run 1（推測） | この節の内訳 |
| L9 | 今の build の E2E-2（en の `L`） | 直接 | L1 | E2E の窓。訳の server は常に `hayamimi-llama-e2e.service`（18084。3.19 の「窓の訳の server」） | 1 run（約 17 分。15 発話 × 5 を ja の 26 分から比で推測） | RP-67 |
| L10 | RP-73（`manifest.rs`、`tests/rp73.rs`） | 直接 | `L` を縮める仕事の後 | cargo | 51 分 | RP-73 |
| L11 | 清書まで入った build での RP-67 と RP-32 の最後の run | 直接 | P3、L17、L24（1000 ms の目標では L7、L8 も） | E2E の窓、GPU。訳の server は常に `hayamimi-llama-e2e.service`（18084。3.19 の「窓の訳の server」） | 約 2 時間（RP-32 の 3 run の実績 71 分と E2E-1、E2E-2） | RP-67、RP-32 |
| L12 | R8x-1：E2E の run の門（RP-83。前の 15 分平均と後の 5 分平均の load、run の間の cargo と rustc）。済み（`7c8e466`） | 直接（E2E の窓の全部の前） | なし | cargo（`hayamimi-eval`） | 60 分 | `rust_port.ja.md` の 3.19 |
| L13 | R8x-2a：`rp67.rs` を作る（済み。19 本、実走の 1 本だけ `#[ignore]`）。`rp79.rs` は止める | 直接 | L12 | cargo（`hayamimi-eval`） | 51 分（`rp79.rs` の 51 分が外れる） | 同上 |
| L13b | R8x-2b：H88 の code（`support/e2e.rs`、`support/rp32_rules.rs` の id、`e2e1.rs`、`e2e2.rs`、`docs/results/e2e/README.md` の有効の文）。E2E の受入が早い規則の run を 300 ms の線で数えて落ちないようにする。済み（`701b483`、fast-forward）。e2e1 22 本、e2e2 15 本、rp32 68 本、rp33 15 本、rp67 18 本、rp80 28 本、rp81 31 本、rp83 54 本が通り、clippy と rustfmt は rc=0。これは検証の席の `--release` の結果（`b973ddb`。`701b483` との native の差は 0 行）で、merge の席は cargo を打っていない（窓 E1 の門を開けるため）。RP-3 の 2 行（T-RP-3-022、023）は `rp03.rs` が無いので未実装のまま | 直接（E2E-1、E2E-2 の受入） | L13（R8x-2a）、L12（R8x-1） | cargo（`hayamimi-eval`） | 51 分（推測）。残り 38 本から外れた（下の数え） | `rust_port.ja.md` の 3.1、H88 |
| L14 | R8x-4：使い回しの鍵の広げ（RP-85）。実装は済み（`88d8283`）。品質の probe と実走（`#[ignore]` の行）は未実施 | 直接 | なし | cargo（`hayamimi-relay`）。品質の probe は窓の席が起こす 18084 の llama-server で約 20 分 | 60 分と 20 分 | 同上 |
| L15 | R8x-5：C と D の offline の門（RP-86 の (0)。今の規則を相手に打ち、D が発話 16 と 22 に効くかを測る）。済み（`03a459b`）。門は落ちた（lat-ja の B だけの割れが W 4 に対し X 6、held-out ja の後ろに声が続く確定が 9 に対し 18、割れが 9 に対し 26）。D は発話 22 を 192 ms 早く確定し、発話 16 には効かない。記録は `docs/results/r8v/extra.md` | 直接 | なし | cargo（CPU） | 60 分 | 同上 |
| L16 | R8x-6：C と D（RP-86、RP-87） | 止める（C と D を両方外した） | — | — | 90 分と 30 分が外れる | `rust_port.ja.md` の H132 |
| L16b | R8x-6b：RP-86 の受入の test（`rp86.rs`） | 止める（同上） | — | — | 60 分が外れる | 同上 |
| L17 | 窓 E3'：E の初回、認識器の初回の復号、鍵の広げが入った 1 つの build で、ja を 2 本、en を 1 本（RP-85 の速さ、RP-84、RP-67 の ja と en）。W と X の対は置かない | 直接 | L5、L14、L13、L13b | E2E の窓と GPU。窓の間はどの席も cargo を打たない。訳の server は常に `hayamimi-llama-e2e.service`（18084。3.19 の「窓の訳の server」） | 約 1 時間 20 分 | `rust_port.ja.md` の 3.19 |
| L18 | R8y-1：`hayamimi` の log の queue（RP-88）。別枠の強化 | 間接。完成までの 39 本の外。E1 の流し直しで止まりが再び出たら完成の条件に上げ、その時は約 111 分と窓 20 分を足す | R8x-3（同じ `log.rs`） | cargo（`hayamimi`、`hayamimi-eval`） | 60 分（推測） | `rust_port.ja.md` の 3.20、H131 |
| L19 | R8y-2：中継の log の queue（RP-88）。別枠の強化 | 間接。L18 と同じ（完成までの 39 本の外。E1 の流し直しで止まりが再び出たら完成の条件に上げ、その時は約 111 分と窓 20 分を足す） | L18 | cargo（`hayamimi-relay`） | 51 分（推測） | 同上 |
| L20 | 済み（`5274a30`）。R11d：rp80 に、CI の `run:` の行で `--test`、`--skip`、`--exact`、`--ignored`、`--include-ignored` が前の語にくっついた形で落ちる検査を足す（RP-80）。くっつくと CI は緑のまま、その binary はほとんど何も走らせない | 間接。完成までの 39 本の外（別枠）。完成の 3 つの条件の合否は検証の席が手で打つ test で決まり、CI の緑を根拠にしないため。ただし同じ欠陥が 3 回出たので、`test.yml` を次に書く slice の merge より前に出す | T の RP-80 の case 行（test-designer の席）。`test.yml` を書く slice とは触る file が違うので並べて出せる | cargo（`hayamimi-eval`） | 30 分（推測）と検証と merge 7〜20 分 | `rust_port.ja.md` の 6 の RP-80、8 の R11d、H133 |
| L21 | R8x-7：句読点の段の温め（RP-84）。main に入った。`Engine::warm_up` が Rz の復号の後に 64 字の文を `Punctuator::restore` に 1 回掛け、`asr.warmup.decoded` に `punct_ms` を足す。受入の判定の文と test の名前は変えない | 直接 | T の RP-84 の case 行（入った） | cargo（`hayamimi-asr`、`hayamimi`、`hayamimi-eval`） | 51 分（推測） | `rust_port.ja.md` の 3.21 の 1 |
| L22 | R8x-8：RP-85 の (4) の数え方。main に入った。発話 7 の標本を製品の `reuse_match` で分け、鍵が合う標本（各 run で 3 以上）だけで訳の段の最大を 182 ms と比べ、合う標本が使い回されたことを確かめる。test だけで、鍵と中継は変えない | 直接 | T の RP-85 の case 行（入った）、設計の merge（入った） | cargo（`hayamimi-relay`） | 40 分（推測） | `rust_port.ja.md` の 3.21 の 2 |
| L23 | R8x-9：RP-83 の判定で、最後の行の `load5` から run 自身の分（`--own-pid` の process の CPU 時間を 5 分の指数で均した値）を引く。main に入った。線 2.50 は変えない | 直接 | T の RP-83 の case 行（入った）、設計の merge（入った） | cargo（`hayamimi-eval`） | 60 分 | `rust_port.ja.md` の 3.21 の 3 |
| L24 | 窓 E3''：L21〜L23 が入った 1 つの build で、ja を 2 本、en を 1 本（名前は `e3q1.e2e1`、`e3q2.e2e1`、`e3q1.e2e2`。RP-84、RP-85 の (4)、RP-83 の en、RP-67 の ja と en）。済んだ（記録は `docs/results/r8x/` の `e3q*`）。`e3p` の記録は上書きしない | 直接 | L21、L22、L23 | E2E の窓と GPU。窓の間はどの席も cargo を打たない。訳の server は常に `hayamimi-llama-e2e.service`（18084） | 約 1 時間 20 分 | `rust_port.ja.md` の 3.21 の 4 |
| O1 | 重ね窓の板（designer）、slice の行を 8 に置く設計【設計の席の返答で直す】 | 直接（goal と FR-9） | 会話字幕の設計 3.12 | 設計の席、designer の席 | 板と設計で約 4 時間（R6 の板から merge までの実績） | 3.12、CS-10〜CS-12 |
| O2 | 重ね窓の crate と CS-10〜CS-12【設計の席の返答で直す】 | 直接 | O1 | cargo、Windows の実機 | 3 slice（推測） | 同上 |
| X1 | RP-69、RP-70、RP-72 の残り、RP-33 の記録 | 間接 | 3.18 の評議 | E2E の窓 | 状態が記録で割れている（PL-H3） | 3.18 |
| X2 | RP-44 と R12y（held-out の入口） | 間接 | なし | cargo | 60 分 | 8 の R12y |
| X3 | Zoom と Teams の実機（CS-17、CS-18） | 間接（実装と偽の受け手の検証は済み） | host の token と主催者の link | 実機 | 打てない | `state.json` の `blocked_on` |
| X4 | SRT の実機、OBS の中の測り、マイクに人が話す確かめ | 間接 | 人と実機 | 実機 | 各 1 回（推測） | 冒頭の表 |
| X5 | 対面のモード（IP-S3 から） | 間接 | R8v の merge | cargo | 記録に見積りが無い | 会話字幕の設計 10 |
| X6 | R3e、RP-1 の ko、T-RP-1-013、RP-10 の `lang` | 間接 | — | cargo、Windows の build | R3e は 45 分 | 8 の R3e、冒頭の表 |
| X7 | `status.md`（英語版）と README をこの節に合わせる | 間接 | この節の merge | main の lock | 1 slice | この文書の冒頭 |

完成の線で直接の物の slice は 44 本、1000 ms の目標では L7 と L8 が加わって 53 本である（P 28、O 4、L 12 と 21 の数え）。
44 本は 2026-10-20 10:30 の数えである。その後、P0（R12a1）、P4（R12a2）、P5 の 2 本（R11、R11c で入った）と L1 の 3 本が済み、Python は 2026-10-20 に消えた（main `9bd6ec3` で `.py` が 0 本）。
残りは 39 本である。44 − 7 に 3.19 の組の 3 本を足して 40 本、L13b（R8x-2b）と L16b（R8x-6b）を足して 42 本、C と D を外して L16（R8x-6）と L16b を除き 40 本、L12（R8x-1、`7c8e466`）が済んで 39 本、L13b（R8x-2b）が済んで 38 本になる。
次の時刻は L13b が済む前の 39 本の数えで、38 本では約 3 時間早い（推測）。39 本を 1 日 8.3 本で割ると 4.7 日で、L12 が済んだ後の 2026-10-21 02:51 から数えて 2026-10-25 の 19:40 ごろになる。L の判定（PL-3）が 1 日遅れて出る分を足して 10-26 の 19:40 ごろ、E2E の窓（E1 の約 20 分と E3' の約 1 時間 20 分）の間に P と O の cargo を止める分を足して 10-26 の 21:20 ごろで、完成は 10-26 の夕方〜夜の見込みである（推測）。
上限「10-26 の夜〜10-27 の未明、悪い側 10-27」は保ち、未明の終わり（05:00 ごろ）までの余裕は約 7.7 時間である。RP-88 を完成の条件に上げると +2.2 時間で 10-26 の 23:30 ごろになる。
この日付は、RP-67 の線が 1600 ms である間だけ成り立つ（`rust_port.ja.md` の H132）。
残りの数えは、この本数の数え方を採る（評議の判定）。今の設計の R13-1〜R13-18 の 19 slice で P1〜P3 を数え直した 32 本は採らない。清書の仕事は R13-13（165 分）、R13-20（40 分）、R13-19（45 分）の 3 slice に分かれ、RP-61 の run は窓 E4 に入る（`rust_port.ja.md` の 8 の R11 の後の組と、上の P3 の行）。1 日 8.3 本は 1 本 51 分（p50）の merge の数なので、本数だけで置き換えると仕事の量を少なく数える。
受付の目標の 10-23 は、39 本を休まず回す時の 1 日 23 本で割った 1.7 日（2026-10-22 の 19:30 ごろ）に、PL-3 の 1 日を足した日である（PL-H2）。
3.19 の組（R8x）は、L12〜L16 の 6 本を足し、L5 の設計の 1 本と L6 の 2 本を完成の道から外したので、L の数は 3 本増えた（窓の L17 は数えない）。方針 B を外したので、L3（51 分）、R8x-2 の `rp79.rs`（51 分）、窓 E2（約 3 時間）の約 4.7 時間の仕事が外れた（`rust_port.ja.md` の H130）。C と D を外したので、L16（120 分）、L16b（60 分）、窓 E3 の W と X の対（約 2 時間 20 分。E3' の約 1 時間 20 分に置き換えて差は 1 時間）が外れた（H132）。
T-RP-32-014 の直し（test-designer の席、約 20 分）は、残っている物の表（P、O、L、X）に行が無く、39 本に入っていない。別に足す仕事で、R8x-1（L12）の前に済ませる。
1000 ms の目標は、L8 の設計、評議、実装 3、RP-66 の run と L7 に約 3 日（推測）を足す。1200 ms と 1000 ms は完成の後の目標なので、合否の日付には入れない。

### 線を 1600 ms にした時に変わる物と変わらない物

完成に要らない仕事は L8（`d` の作り直しの設計、評議、実装 3、RP-66 の run）と L7（RP-74 と RP-78。縮みは復号の 32 ms）で、どちらも目標の道に残る。
要る仕事は、L1、L2、L5、L9〜L15、L17、P0〜P5、O1、O2 である。L3、L4、L16、L16b は止めた。L6（推測デコード）は完成の道に置かない（3.19）。
1600 ms は悪くなることを止める線で、速さの目標ではない。1200 ms に届かせる手（無音で閉じた 39 標本を縮める物）と、「割れは W 以下」と 1200 ms を両立させる detector の形は、完成の後の別の設計に置く。

Python を消す仕事の量が線で変わらないことは、次の 2 つで確かめた。

- 3.2 の R11 に入る 3 つの条件の節（R11 の入口の条件と 78 本の表）に `RP-67` は 0 件、`RP-19` は 3 件在る。
- 6 と 7 の RP-9、RP-19、RP-61 の 6 行に `RP-67` も `1000 ms` も 0 件である。

RP-61 の合否は Python の `L` と、清書を切った Rust の `L` との差（p50 30 ms、p95 60 ms）で、RP-67 の線を読まない。
ただし RP-67 は清書まで入った build で測り直す（L11）ので、清書の chain（P3）はどちらの線でも RP-67 の合否より前に入る。

### 方針

順位は上の「順位」のとおりに決まっている。
方針の違いは、3 位の `L` の仕事を、1 位と 2 位の間のどこで動かすかである。

| 方針 | 形 | Python が消える日 | 1000 ms の目標に届く日 | 1600 ms の線の完成 | ただし | 採るか |
|---|---|---|---|---|---|---|
| A | P、O、L を 1 本ずつ順に出す（P0〜P5 の後に O1、O2、その後に L1〜L11） | 2026-10-20（済み） | 2026-10-29 | 2026-10-26 | P の間は重ね窓の板も動かず、O が P5 の後ろに乗る。L の判定（PL-3）が 10-25 まで出ず、L8 が要る時の設計と評議が終わりに乗る | 採らない |
| B | P と O を並べ、2 つが main に入ってから L を出す | 2026-10-20（済み） | 2026-10-28 | 2026-10-26 | PL-3 が 10-24 まで出ない。L1 の 3 本の branch が 4 日 main から離れ、`hayamimi-asr` を触る P2 の R12h、R12v と衝突を重ねる | 採らない |
| C | 席、cargo、main の lock は P と O が先に取り、L は P と O が使わない資源（E2E の窓と GPU、L2 の CPU の probe）でだけ進める。L1 の 3 本の merge は main の lock を P0 の後に取る（済み） | 2026-10-20（済み） | 2026-10-27 | 2026-10-26 の夕方〜夜（上限は 10-26 の夜〜10-27 の未明、悪い側 10-27） | P と O が席を全部使う日は L が 1 本も進まず、PL-3 が 10-23 より遅れる。その分だけ 1000 ms の日付が延びる | 推す |

推す形は C である。
C は順位を崩さずに、GPU と E2E の窓（P と O の slice はほとんど使わない）で E1 と E3' を流すので、1600 ms の線を 3.19 の組が通るかを 10-23 に測れる。
通らない時の評議を、P と O の終わりより前に始められる。

### 方針 C の段

| 段 | 終わり | 1 位と 2 位（先に席を取る） | 3 位（残りの資源だけ） | 順にする物 | 段の終わりの測り |
|---|---|---|---|---|---|
| C1 | 2026-10-21 | P0、P4、P1 の 6 slice の書き始め、O1（板と設計）【設計の席の返答で直す】 | L1 の 3 本の merge は済み。L2（CPU）、L12、L13、L14、L5（触る crate が違うので並べる） | P0 と L1 は済み。L12 は E2E の窓の全部の前 | PL-1、PL-2 |
| C2 | 2026-10-22 | P1 の残り、P2 の前半、O2 の 1 本目【設計の席の返答で直す】 | L15 は済み（門は落ちた）。L13b、窓 E1（L9）。L3、窓 E2（L4）、L16、L16b は止める | `session.rs` は R12b、R12h、R12i の順。窓の間はどの席も cargo を打たない（RP-83） | — |
| C3 | 2026-10-23 | P2 の後半、P3 の R12w0（済み）と R13-13、O2 の残り | 窓 E3'（L17。ja 2 本と en 1 本、約 1 時間 20 分。E2E の窓と GPU。窓 E4 より前） | E2E の窓は E1、E3' の順。窓は cargo を止めて 20 分以上たってから開く | PL-3。超えれば評議へ出す |
| C4 | 2026-10-24 | P3 の R13-13、R13-20、R13-19。重ね窓の CS-10〜CS-12。P5（R11）は 2026-10-20 に済み（R11c） | L5、L10。1000 ms の目標では L8 の設計と評議 | RP-61 の run は窓 E4（L11）の中で取る | PL-4。Python が消えたこと（`rp09`）は 2026-10-20 に満たした |
| C5 | 2026-10-26 の夕方〜夜（上限は 10-26 の夜〜10-27 の未明、悪い側 10-27。1000 ms の目標は 2026-10-27） | — | L11。1000 ms の目標では L8 の実装と L7 | E2E の窓は L11 を最後に 1 回 | PL-5 |

### 段の測り

| ID | 測る物 | command | 期待 | 陽性対照 | 陰性対照 |
|---|---|---|---|---|---|
| PL-1 | Python を消す門が立った | `cd native && cargo test --release --jobs 2 -p hayamimi-eval --test rp19` | `rp19_python_cases_are_ported` が未実装の数を出して落ち、`rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks` が通る | 一覧の数が 172 以下 | 一覧に在る名前を list に足した fixture で 2 本目が落ちる |
| PL-2 | 方針 B の offline の門 | `cd native && cargo test --release --jobs 2 -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_latency_set_probe_shadow_rule` | H100 の通る線を満たす | 今の判定の probe の値が同じ log に在る | — |
| PL-3 | 1600 ms の線を 3.19 の組が通るか | 窓 E3' の ja の 2 本と en の 1 本の `jsonl` を `cd native && cargo test --release --jobs 2 -p hayamimi-eval --test rp67 -- --ignored` に読ませる（`rp67.rs` は入った。窓 E3' の記録は `docs/results/rp67/` に置く） | ja と en の `L` の p95 が 1600 ms 以下。1200 ms を超える標本の数も目標の側の記録として出す。1600 ms を超えれば、外れた標本の発話と段を添えて評議へ出す。線は動かさない | 今の記録（`rs1`〜`rs3`）で 1501 ms と出る | 全標本 900 ms の fixture で通る |
| PL-4 | RP-19 の残り | 同じ `rp19` | 一覧が R12w の 17 行だけ | 同上 | 同上 |
| PL-5 | 完成 | `rp67`、`rp73`、`rp19`、`rp09` と CS-10〜CS-12 の test | 全部通る | RP-67 と RP-73 の行の陽性対照 | 同じ行の陰性対照 |

### 計画の残る穴

- **PL-H1** 完成の 3 つ目（重ね窓）は、この節より上の完成の条件の文に無い。順位の 2 位に置いたので、この節は完成の条件に入れて数えた。上の条件の文を直すのは X7 の席である。
- **PL-H8**（決定済み）残りは 39 本である（L12 が済んだ）。E2E の窓（E1 と E3' で約 1 時間 40 分）の間は、P と O の cargo の席も止める。方針 B を組から外したので窓 E2（RP-79 の run）は止め（`rust_port.ja.md` の H130）、C と D を外したので窓 E3 の W と X の対と L16、L16b は止める（H132）。1600 ms の線の完成は 2026-10-26 の夕方〜夜で、上限「2026-10-26 の夜〜2026-10-27 の未明、悪い側 10-27」は保つ（推測）。日付は線が 1600 ms である間だけ成り立つ。
- **PL-H9**（NOT VERIFIED）1600 ms の余裕は 1 つの記録（`e-card-form` の `rs1`〜`rs3`、291 標本）で数えた。発話 16 の再生の最後の `final`（590 ms）と live の `d`（平均約 1038 ms）が合わない理由と、R8x-5 の機構の対照を held-out で打っていないことも確かめていない（`rust_port.ja.md` の H132）。
- **PL-H7** P0〜P5 と O1、O2 の行の数と量は、Python を消す設計と重ね窓の設計が入る前の記録で数えた（印【設計の席の返答で直す】）。2 つの設計が slice を増やすか減らすと、1 本につき約 3 時間（1 日 8.3 本）ずつ、Python が消える日と完成の日が動く。直すのは、2 つの設計の席の返答を受けた設計の席である。
- **PL-H2** 1 日 8.3 本は 10-16〜10-18 の 0 本の 3 日を含む平均である（実測）。毎日 23 本（休まず回す時の速さの見積り）なら、2026-10-21 02:51 に残る 39 本で 1600 ms の線の完成は 2026-10-23 になる（PL-3 の 1 日を含む。受付の目標）。動かない日は 1 日ずつ後ろへ延びる。
- **PL-H3** RP-71 は、記録を読む test と S3、N0 の計測の記録（`c7decfb`、`7522f58` で merge）と訳の試験用の llama-server の unit（`38e54e7`、`0f79478` で merge）が main に在り、冒頭の表の RP-71 の行をそれに合わせた。速さの数は原因分析の席が数えている。RP-69、RP-70、RP-72 は、冒頭の表で「case 行を書いている」だが、`test/rp69-72` は 10-19 に main へ入っている。どこまで済んだかは確かめていない（NOT VERIFIED）。持ち主は 3.18 の評議の席である。
- **PL-H4** `L` の埋め方の表は、1 つの build の 3 run（ja だけ）の標本の段を置き換えた計算で、段の間の待ちの干渉と load は入っていない（ASSUMPTION）。en は今の build で測っていない（L9）。
- **PL-H5** RP-71 の訳の縮み、L5 と L8 の量、重ね窓の量は記録に実績が無い（推測）。L8 は評議で止まりうる。R8v の設計（3.17）は merge から 5 日で実装に入っていない。
- **PL-H6** 英語版 `status.md` に、この節の訳を足した。2 本は、日本語を先に直して揃える。
- **PL-H10** L21〜L24（3 slice と窓 E3''）は、残りの本数と完成の日付の数えに入っていない。足す量は slice が約 2 時間 30 分（51 分、40 分、60 分。推測）と窓の約 1 時間 20 分で、L11（窓 E4）は L24 の後になる。評議は 3.21 の 2 を採り、3 を `llama-server` の pid を cmdline の `--port 18084` で確かめる形を足して採った（`rust_port.ja.md` の H141）。L21〜L23 は設計の merge の後に出せる。数え直すのは、計画の数を次に書く席である。

## slice ごとの状態

状態の語は 5 つである。

- **済み**（表では「検証済み」とも書く）：merge され、書いていない席の検証が可を出した。
- **実装済み・検証待ち**：branch に commit してあり、merge はまだで、書いていない席の検証がまだ済んでいない。
- **作業中**：branch か worktree で作っている。
- **計画済み**：まだ作り始めていないが、`rust_port.ja.md` の 8 の波に入っている。
- **未着手**：まだ作り始めておらず、波にも入っていない。

| slice | 状態 | merge の commit | 検証の出所 | 受入の通過 | 残る穴 |
|---|---|---|---|---|---|
| 会話字幕 slice 1（`hayamimi-relay`） | 済み | `08cfdd4` | `rust_port.ja.md` の 8（「済んでいる」） | 会話字幕の設計の順 1〜13 | 会話字幕の設計 9 の残る穴 |
| R0（Python の基準と `hayamimi-eval`） | 済み | `5bb6343`、`228a88b` | `5bb6343` の commit の 1 行目（「verified (RB-1..8)」） | RB-1〜RB-8 | `rust_port.ja.md` の H14、H26 |
| R1（計測の slice） | 済み | `1b93167` | `1b93167` の commit の本文（RC-1〜RC-4 の出力） | RP-17、RP-24（`83af5a5` で verified）。RP-7、RP-8、RP-20、RP-23（Windows の `--ignored` で pass） | H29（負荷の分け方、訳 T の測り直し）、H30（fp16）、H31（R7） |
| R1b（評議と仮の値） | 済み | D `68867d8`、T `92f2641`、test `d921b21`、結果 `47341b4` | 評議の判定（git の外）を D に写した `68867d8` | 対の比較の 20 / 23 と根拠 3 の 1% を実測で裏づけ、`--min-silence` を 0.30 に決めた | H8（2000 ms、20%、14,000 MiB は据え置き） |
| R2（ja の経路、`--wav`、`/events`、CPU への fallback） | 済み | `f0aeaf4` | 独立検証の 2 回目で可（検証の席の返答、git の外）。直しは `a8840a7`、RP-6 の受入の置き直しは `1d33a5b` と `255c6a0` | `a8840a7` の本文の WSL の run で rp06 3、rp16 17、rp26 12、rp29 22、rp02 10、rp18 8、rp27 7 が pass。T-RP-6-012 は RP-6 の置き直しの後に通った | H8c（RP-6 の閾値）、H33（CPU に落ちた句読点の一致の件数）。T-RP-21-001、005、006、007 は R10 |
| R3a（言語の振り分け。`hayamimi-asr` の `route` と `lid`） | 済み | `7d82842` | 独立検証で可（検証の席の返答、git の外） | 通過は RP-16 の言語判定の行（T-RP-16-018〜026）と、RP-29 の R3 の行の fixture（T-RP-29-022〜026）。RP-1 は R3b を待つ。`route.rs` と `lid.rs` の RP-19 の名前 79 本が、台本の認識器と言語判定に対する単体 test として pass。RP-16 の言語判定の行は、`rp16.rs` の fixture T-RP-16-018〜022 と、`hayamimi-asr/tests/rp16_lid.rs` の model を使う実走 T-RP-16-023〜026（CPU、GPU、model が無いとき）。`hayamimi-asr/tests/gpu_guard.rs` の T-RP-16-027〜030（device を隠して `Gpu::Try` で開くと、言語判定と句読点が CPU に落ちて process が生きる。device の数を数えない run は終わる）。`lid_dual.rs` の T-RP-16-031〜034（golden の 8 本で、whisper-tiny 単独は 6 本が ja、二重確認の後に残る言語は 8 本とも ja）。RP-29 の R3 の行は fixture だけ（T-RP-29-022〜026）。RP-1 は未実行（R3b の `scorecard` が要る）。RP-29 の R3 の行の実走は無い（`hayamimi` の binary がまだ振り分けない） | `Decode` trait は R3b の merge まで台本で代用。言語判定はまだ `hayamimi` の binary に繋がっていない（`Engine::open` が `lid::open` を呼ばない。持ち主は R3b、T-RP-16-035）。CUDA と ORT の session を同じ process で同時に開くと abort することがある（並列 2 の 9 回で 2 回）ので、R3b が `hayamimi-gpu` に session を作る所の process 全体の lock を置く（T-RP-16-036、037）。`--lang auto` は binary を繋ぐまで使えない。ko の分かち書きとユーザーの辞書は振り分けの後ろに置く |
| R3b（認識器、常駐、`scorecard`、`lat-multi`、言語判定の結線、session の lock） | 検証済み | `0358773` | 独立検証で可、`1189d91`。`8fe1f19` の set ごとの session の reset を含む（検証の席の返答、git の外）。main の上で rp30 12、rp08 31、rp20 11 を打ち直して通過 | 2446 passed、0 failed、clippy 0、fmt 0。rp30 12、rp31 29、rp32 2。RP-1：ja の LID 15/15、ja_02 は ja（tier rz）。RP-22 の run：peak 4552 MiB、起動前の 3 回の読み 1000 MiB、llama-server なし。未実行は merge 後の木での T-RP-22-007〜011、llama-server を起こした 007（E2E の波）、新しい陰性対照での T-RP-1-013、`lang` の条件つきの RP-10（Windows） | H13、H24、H25、H35（ko） |
| R3c（配る形の build の RP-1 の zh と yue、H35） | 検証済み | `b8fd07e` | 独立検証で可（検証の席の返答、git の外） | 振り分けを通した Windows の配る形の build の run で、zh と yue は Python と同じ値。ko の 86939 ppm は R11 の入口の上限（RP-36）。未検証は `cli.rs` の先読みの 4 行の Windows の target での compile と、T-RP-1-013 の実記録（持ち主は E2E の席） | H35（ko は 0.081 に対して赤のまま） |
| R0b（worktree から本体の `testdata/` を引く test の support） | 済み | `32ecb2a` | 独立検証で可（検証の席の返答、git の外） | `testdata/` の無い worktree で `hayamimi-eval` の test が FAILED 0。陰性対照は、探し先を両方とも無い path にすると、その 2 つの path を名指しして落ちる。symlink は 0 件 | 未実行：本体にも worktree にも `testdata/` の無い clone で落ちること（`rust_port.ja.md` の 8 の文言）。検証の席は、2 つ目の探し先を差し替える形で同じ落ち方を確かめた |
| R4（話者） | 済み | `ff85b01` | 独立検証で可、`48a21f0`（検証の席の返答、git の外） | `cargo test` は FAILED 0。`--include-ignored` の実走で rp15 が 13、rp15_speaker が 5、rp16 が 37、rp29 が 32 passed。T-RP-15-012 の順序の陰性対照あり。R5 の rp10、rp11、`--source` は通ったまま。clippy 0、漏れ 0 | `notice.rs` の `STAGES` と `REASONS` を `rust_port.ja.md` の 3.15 の一覧と突き合わせる test はまだ無い（持ち主は R10） |
| R5（取り込み、速報、server の全 route） | 済み | `2efbd5c` | 独立検証で可、`9a3f512` と `0767efb`（検証の席の返答、git の外） | `9a3f512`：rp10 が実走を含めて 13 passed（`lang` が `en` の条件は R3b を待つので除く）、`--source` が 2 passed、陰性対照あり。`0767efb`：`cargo test` が FAILED 0、rp11、rp29、Windows の build | NOT VERIFIED：Windows の実機での mic と loopback（E2E）。Linux の build に `libasound2-dev` が要る（`cpal` が `alsa-sys` を引く。`docs/guide/run_hayamimi.ja.md`） |
| R6（Dioxus の 3 頁、`/ui` の結線） | 済み | `7446f75` | 独立検証の席、画面テストの席、デザイナーの席（返答は git の外） | 検証：`cargo test` は FAILED 0 で 419 passed、W7 の陰性対照で 3 本が落ちた、clippy 0、衝突 0、漏れ 0。画面テスト：BD-1〜11 と W7 が通過、header は 60.80 px。BD-12 は通過（デザイナーの席。差 6 つは全部「許す」、「直す」は 0 件） | NOT VERIFIED：fallback の帯が在る時の DOM（帯の高さと `role="status"`）。E2E の画面の確かめで測る |
| R7（認識の遅れの詰め） | 済み。道具と test は検証済みで、置き場の測り直しの結果、認識は CPU（int8）のまま | `f9aaeb9`。測り直しの記録は `docs/results/r7/remeasure_20261015/` | 独立検証で可、`646a786`（検証の席の返答、git の外） | `cargo test` は 990 passed、FAILED 0。rp04 は 8 passed、1 ignored。rp07 は 9 passed。陰性対照 2 本、clippy 0、漏れ 0。測り直し（2026-10-15 04:45〜04:55、測りの席の申告）：recognize_total の p50 の交互 3 回の中央値が CPU int8 173544 µs、GPU fp32 229705 µs、CPU fp32 200120 µs。どの対でも CPU int8 が速く、置き場は評議へ戻さない。load 2 未満で始まったのは 1 回目（1.98）だけで、以後は 3.8〜11.4 なので、値は NOT VERIFIED、向きは使う | H9、H10、H41（持ち主は R8h の後の席、E2E の後）、H31（置き場に効かないので調べない） |
| R8a〜R8g（訳の中継の Rust の `hayamimi` への接続、訳の速度、外の送り先） | R8a: 検証済み（独立した席: CS-21 と CS-22 は陰性対照つきで通過。実音声 10 分の実行は E2E が受け持つ）。R8b: 検証済み。R8c: 検証済み。R8d: 検証済み。R8g（Zoom、Teams、SRT）: 検証済み。R8e: 検証済み（T-RP-5-009 は落ちたと記録）。R8f: V の直しと 2 本目の server を含めて検証済み | R8a: `c868eb0`。R8b: `c663ce6`。R8c: `9278273`、chat の endpoint の直しは `022b283`。R8d: `57e4d6a`。R8g: `a496066`。R8f: `5861235`（R10 と一緒に fast-forward）、V の直しと 2 本目の server は `a5bccb0`、`fleurs-pairs --split` は `ee57b26`、`vprobe` は `4082d19`。`live` の `translate.fallback.held` は `0a52cfd`。R8e: `14c28fd` | R8a: 独立検証で可（検証の席の返答、git の外）。R8b: 独立検証で可、main の上で打ち直し（検証の席の返答、git の外）。R8c: 独立検証で可、統合の水準の陰性対照 2 本つき。chat の endpoint の直しも別の独立検証で可（検証の席の返答、git の外）。R8d: 独立検証で可、`e81451f`（検証の席の返答、git の外）。R8g: 独立検証で可、`faa8fa2`（検証の席の返答、git の外）。R8f: 独立検証で V1、V2、V4〜V7 が可、`verify/w3-r10-r8f` で R10 と合わせた INT-1〜5 が可（検証の席の返答、git の外）。V の直しと 2 本目の server は `verify-w3` で合わせて検証して可（検証の席の返答、git の外）。`--split` は SP-1〜4 が可、`live` の held の event は H45-V1〜V5 が可、`vprobe` は VP-1〜4 が可（検証の席の返答、git の外）。R8e: 独立検証で R8e-V1〜V12 が可、V9（手順書の 298 行目）は `474a222` で直した（検証の席の返答、git の外） | R8a: 会話字幕の CS-21 と CS-22 が偽の recognizer と陰性対照つきで通過。R8b: CS-20 の 001〜004、RP-16 の訳の行（T-RP-16-038〜050）、GPU 確認の陰性対照、既定の port 8080 が通過。main の上の `cargo test --release --no-fail-fast` は FAILED 0。R8c: CS-5、CS-25、RP-25 の 001〜009、RP-29 の sink の場面（偽の送り先）、T-CX-1-002 と 006 が通過。RP-25 の 010 は RP-3 の run（R8e）を待つ。R8c の直し：8080 の server に向けた probe1 の 40 行で、中継と curl の違いが 0 行、旧い `/completion` の形では 40 行とも違う。probe の log との一致は 40 行中 38 行。body を比べる test と陰性対照が通過。`timings.cache_n` を確かめる test はまだ無い。R8d: 977 passed、FAILED 0、陰性対照 2 本、clippy 0、衝突 0、漏れ 0。未実行は RP-3、RP-13、RP-28。R8g: too_open の test は event が出ないと落ちる（陰性対照つき）。srt の test 7 本が通過。Windows だけの test 3 本が compile される（壊れを仕込んで確かめた）。clippy 0、衝突 0、漏れ 0。R8f と R10 を合わせて 2249 passed、0 failed、clippy 0。R8f の V の直し（`d888f87`）と 2 本目の server は 2273 passed、0 failed、clippy 0、fmt 0、main の上で rp30 12、rp08 31、rp20 11 が通過。V3 は port 18081 の 2 本目の llama-server（`native/hayamimi-relay/units/hayamimi-llama-other.service`）で通過（zh の確定の訳 1 件）。`--split` は `-p hayamimi-eval` が 744 passed、0 failed、rp30 12。`live` の held の event は 2278 passed、0 failed、陰性対照 3 本が狙いどおり落ち、clippy 0、fmt 0。`-p hayamimi-relay` は 599 passed、0 failed。`vprobe` は relay 614 passed、0 failed、rp08 31、rp20 11。dev の seed 0 の止まる行 70→30 を再現（調整の組で held-out ではない）。R8e: RP-3 の Rust の run を `docs/results/r8e/` に記録。stream の接続を閉じると llama-server の slot が 48 ms と 67 ms で空く（H6 を閉じた）。RP-5 の基準は `--min-silence` 0.30 の Python の run | H17、H32、H8b、H36、H44、H46。R8b の未実行は T-RP-16-051（GPU のある機械で打つ）と CS-20 の 005（E2E の波が受け持つ）。R8d の未実行は T-RP-5-009 の実走（R8e）、`translate_partial` の打ち切りの実走（H6、R8e）、`show_partial` だけの陰性対照。R8g の未実行は実機の run（英語、翻訳器、GPU、長い動画）、8 秒静かなら集め終える判断、`/ingest` の client に `session_start` が届くか、CS-17 と CS-18、Windows での実行。R8f の未検証は、V の直しの held-out での効果（H36。test split の 1 回だけの測定は検証の席 `aa4bb33` が持つ）、RP-13 は R11 の入口で未通過のまま、live の ja→en の `L`（実装の席の 1 回の run の申告で、打ち直していない）、`execute` が `args.split` を `fleurs::run` へ渡す所を通る unit test が無いこと（読んで確かめただけ）。5 秒ごとの `/health` の確かめは作っていない（H46）。R8e: T-RP-5-009 は実測で落ちる（Rust の p95 4398 ms 対 Python 3751 ms、H44）。RP-29 の負荷の 1 件は原因が未特定で落ち、検証の席の打ち直しでは 4 本とも通った |
| R10（取得、文書、英語版） | 検証済み | `5861235`（R8f と一緒に fast-forward） | 独立検証で R10-V1〜V8 が可、`verify/w3-r10-r8f` で R8f と合わせた INT-1〜5 が可（検証の席の返答、git の外） | R10-V1〜V8。R8f と合わせて 2249 passed、0 failed、clippy 0 | H16 |
| R8h（`L` の差の詰め、3.16） | 1 と 3 は検証済み（RP-31 の数え方、RP-32 の訳の取れた割合の判定、`asr.feed.waited` の log、`<run>.gpu.json` と `gpu_load::pair_is_valid`）。2 の B は R8f の V の直し。4（RP-32 の交互の run）は E2E の波で打つ | `7399db8` | 独立検証で R8h-V1〜V9 が可（検証の席の返答、git の外）。`verify-w3` で rp08 31、rp20 11、rp31 29、rp32 2 を打ち直して通過 | 2316 passed、FAILED 0、clippy 0、fmt 0。R8e の記録を数え直すと n 67、`L` の p50 1533 ms、p95 2752 ms、有効 100 のうち訳あり 66 で 20/23 を割る（検証の席の独立の式）。未実行は RP-32、RP-33、RP-34、RP-35、`gpu.json` を書く `stream` の実走 | H42、H43、H44。`r8a_follow` が AddrInUse で 1 回落ちた（単独の 3 回は通る）、原因は未特定 |
| R8i（zh、ko、es の推量の不一致を log にだけ残す、3.6） | 検証済み。R8j（RP-38 の回帰と hub の test の直し）も検証済み | `aa85fb4`。R8j は `7f29938`。R8k（RP-36 の 005 と 006、RP-37 の 008〜017）は `6cf71ff` | 独立検証で V1〜V7 が可、`a730738`（検証の席の返答、git の外）。merge 後の木で relay 621 passed、0 failed、rp08 31、rp20 11、rp36 3 passed（1 ignored）、rp30 12 を打ち直し。R8j：独立検証で V1〜V6 が可、`a730738`（検証の席の返答、git の外）。merge 後の木で relay の binary 37、ok 行 37、623 passed、0 failed、rp08 31、rp20 11 を打ち直し。R8k：独立検証で V1〜V4 が可、`a53f30b`（検証の席の返答、git の外）。merge 後の木で `-p hayamimi-relay -p hayamimi-eval` が binary 64、ok 行 66（binary 64 と Doc-tests 2。`rp23` の入れ子の 4 行は数えない）、1433 passed、0 failed | RP-37 が通過。R8j：RP-38 が通り、陰性対照（`settle_other` で V を 2 回）は 2 対 1 で落ちる。hub の test は id を読んだ所で読み止め、枠の `\n\n` の前で比べていたのが原因で（H49 を閉じた）、単独 10/10 で通り、陰性対照は 5/5 で落ちる。R8k：12 本の名前が T と一致し、陰性対照（011 の変更）で 12 行が落ちる。T-RP-37-012 は `rp37.rs` の入力に揃えた。zh seed 0 の否定の壊し行 9/13→8/13（id 1512）は H36 の免除。未実行は、`pairs_seed1` が dev から seed 0 を除いた組であることの照らし（RP-37 の行） | H36、H48、H50、H52（全体の run で `cs28` が 1 回落ちた）。`aa85fb4` の全体の run は 2483 passed、0 failed、87 ignored。検証の席の 2471 と 86 は、`rp11` の `test result:` の行が log の行で 2 行に分かれ、1 行を拾えなかった数え |
| E2E（端から端、2026-10-15） | 単独の 1 run で E2E-1 と E2E-2 の受入 test が PASS。評議の判定で、E2E-1 の正式な判定は RP-32 の交互の 3 対で出す。結果は冒頭の表の RP-32 の行 | `dfa85d6`（`feat/r8n` の merge）。記録は `9103c95` の `docs/results/e2e/`（`e2e1/e2e1.jsonl`、`e2e1/py1.jsonl`、`e2e2/e2e2.jsonl`）。run の build は main の `7ea558f` | 受入 test `e2e1_mic_process_ja_to_en_beats_python` と `e2e2_loopback_process_en_to_ja_within_budget` の PASS は実装の席の申告で、設計の席は打ち直していない。数は設計の席が `9103c95` の jsonl を jq で引き直し、`docs/results/e2e/README.md` の表と一致した | 有効は 3.1 の式で `--min-silence` 300 ms、`L` は有効のうち訳の取れた標本で取る（p は nearest rank）。E2E-1（`lat-ja`、23 発話 × 5）：Rust は有効 100、訳あり 93、`L` の p50 1415 ms、p95 1872 ms。Python（`py1`）は有効 100、訳あり 95、p50 1478 ms、p95 2187 ms。E2E-2（`lat-en`、15 発話 × 5、loopback の送り先）：有効 75、訳あり 70、p50 970 ms、p95 1303 ms。遅れの内訳：VAD が区切りを決めるまでの待ち d（`L_asr − L_dec`、有効な標本）は、ja（E2E-1 の Rust）が p50 865 ms、p95 1341 ms、en（E2E-2）が p50 456 ms、p95 706 ms。d は評議の席の申告を設計の席が引き直した値である。`L` はこの待ちを含み、`t_end` の定義（manifest の発話の終わりの sample を送った時刻、3.1）は変えない。RP-22 は別の run（`docs/results/r3/rp22_run.json`）で通過：peak 8936 MiB（上限 14000）、起動前 7443 MiB、memory の落とし 0。RP-5 は打ち直していない（前の run で速報の p95 が Rust 3737 ms 対 Python 3265 ms、H44） | RP-32 の品質の差の原因。RP-5 の打ち直し。R8u（作らない）、R11、R12y は別の行で扱う。H53、H54、H55 |
| R8m、R8o、R8p（E2E の後：`listen` を SSE で、両方止めた行の観測と否定の表、E の雛形を剥がす） | 検証済み | R8m は `295fed8`。R8o は `7b3544c`。R8p は `cd91986` | どれも独立検証で可（検証の席の返答、git の外） | R8m：eval の Running 29＋Doc-tests 1＝ok 行 30、813 passed、0 failed（RP-41）。R8o：relay の 39＋1＝ok 行 40、653 passed（RP-42、RP-43）。R8p：relay の 40＋1＝ok 行 41、675 passed、0 failed（RP-45。T の 4.55 の case 行は test-designer の席が書き直すまで暫定）。E2E-1 と E2E-2 は 3 つとも入った `7ea558f` の build で打ち直した（通過の出所は E2E の行） | H58（本物の `/events` を Windows で）。続きは R8o2 と R8p2 の行 |
| R8o2、R8p2（語幹が「な」の 4 語を否定の `exclude` に、札の読みを行頭に狭める） | 検証済み | R8o2 は `521a9cf`（commit `c133eed`）。R8p2 は `dfd68df`（commit `0678102`） | どれも独立検証で可（merge の commit の 1 行目の「verified」。検証の席の返答は git の外） | R8o2：RP-43（T-RP-43-005）。R8p2：RP-45（T の 4.55 は `38f1a73` で書き直し、010〜016 は未実装）。数は検証の席の返答に在り、この文書は打ち直していない。`7b3544c`〜`dfd68df` の間の main では `-p hayamimi` の test が build できなかった（`native/hayamimi/tests/r8a_relay.rs` の `RelayConfig` に `verify_dump` が無く E0063）。直しの `a6d2d5b` は main に入っている。workspace の test は `fccce49` の木で Running 119＋Doc-tests 7＝126、ok 126、FAILED 0（`state.json` の記録）。E2E-1 と E2E-2 は 2 つとも入った `7ea558f` の build で打ち直した（通過の出所は E2E の行） | H56（`--verify-text` なしの時の test）、H59（「〜なさそう」の推量）、H60（行頭に無い札と綴りゆれの代償）、H61（英語の接頭辞の否定） |
| R8t（en→ja の推量を片向きに、義務の過去形と「なくては」の 8 つを否定の `exclude` に） | 検証済み | `e55d8cf` | 独立検証で可（merge の commit の本文：720 passed、`rp63` 5 passed、衝突 0） | RP-63 | H53 の 3 |
| R8u（T と E の両方が数で止まった行を 1 回訳し直す）、R3e（session の言語の文字を持たない 1〜2 字の確定文を捨てる） | R8u は作らない（probe で訳し直しが効かず、その行は held のまま原文の字幕を出す。RP-64 は欠番）。R3e は未着手 | なし | なし | RP-65 は未実行 | H66、H67 |
| R8s（ja→en の推量を log にだけ、英語の時刻、E2E-1 の `--verify-text`） | 検証済み。RP-49 の D の文は実装（半時の語に 30 を足す、ja と en の両方向）に合わせて直した | `a557a96` | 独立検証で可（merge の commit の本文：714 passed、衝突 0） | RP-49 は再現の行（R8s の前の main で `At one thirty` が止まる）が落ち、実装で通る。数は検証の席の返答に在り、この文書は打ち直していない | H59 は R8s の 1 が入ると閉じる（R8s は入ったが、`rust_port.ja.md` の H59 の行は書き替わっていない）。H53 の 3 |
| RP-32 と RP-33 の test、R8p3 | 検証済み | RP-32 の名前の test と R8p3 は `e745bd3`、RP-32 の `pairs.txt` の dir を読む test は `0900a32`、RP-33 の test は `fccce49` | 3 本とも独立検証で可（merge の commit の本文）。workspace の test（`-- --skip rp19`）は `fccce49` の木で Running 119＋Doc-tests 7＝126、ok 126、FAILED 0（`state.json` の記録） | `native/hayamimi-eval/tests/rp32.rs` と `rp33.rs` が main に在る。R8p3 は RP-48 に合わせて RP-42-006、RP-43-011、RP-43-012 の期待値を直した（`445aa2f`） | RP-32 と RP-33 の実走は 2026-10-16 の Rust の 3 対で打ち直した（結果は冒頭の表） |
| E の card の形、早い終わりの保留の `reason`、初回の依頼（`feat/e-card-form`） | 検証済み | `83830af`（`1b205e0` と `0aadb6c` を含む） | 独立検証で可（検証の席の申告、git の外）。merge の commit の本文に workspace 全体の test の結果は無い（push の前に受付が打つ） | E の依頼は system（T の下書きと、事実、数、固有名、否定、推量を保つ注）と user（T と同じ文）の 2 つで、T の依頼は user 1 つのまま。`asr.endpoint.held` は `reason`（文が完結していない `incomplete`、声が短い `too_short`）を持つ。`live` は `relay.serve.started` の前に、`--llm` の server へ短い依頼を 1 本だけ送り（次の行で 2 本になった）、返れば `translate.warmup.sent`、返らなければ `translate.warmup.failed` を出して起動は続ける。検証の席の申告：4 crate の test が 2688 passed、0 failed、clippy と fmt は rc=0、陰性対照 5 本が落ちる。実測（3 run とも同じ）：`asr.endpoint.held` が 77 件（`incomplete` 68、`too_short` 9）、`translate.warmup.sent` が 1 件 | `rs1` の最初の E が 1326 ms（初回の遅さ。持ち主は実装の席）。発話 22 が `incomplete` で 3.3 s 持たれる原因（持ち主は root-cause の席）。`incomplete` を数える目的は、名詞止めの扱いを決める材料にすること（`rust_port.ja.md` の 3.17） |
| 初回の依頼を T の形と E の形の 2 本にする（`feat/e-warmup`） | 検証済み | `00087bb`（`2e82d3c` と `c2eaa03` を含む） | 独立検証で可（検証の席の申告、git の外）。merge の前の木（`8bd533b`）の test は受付の実測で rc=0、results=115、passed=2694、failed=0、ignored=116（log は repo の外）。`FAILED` の字を含む 45 行は test の中の log の行で、test の失敗ではない | `live` は `relay.serve.started` の前に、T の形、E の形の順に短い依頼を 1 本ずつ送る。待つのは 2 本で合わせて最長 10 秒で、2 本目は残りの時間を受ける。依頼ごとに `translate.warmup.sent` か `translate.warmup.failed` が出て、どちらも `form`（`t` か `e`）を持つ。陰性対照：2 本目の待ちを固定の 1 ms にすると 2 本が落ちる（前回は 0 本） | NOT VERIFIED：実機の llama-server で E の最初の 1 回が縮むか（次は E2E の 3 run で測る）。極端な負荷での余裕 1 s の耐性 |
| 方針 B の設計（RP-79、`design/vad-end`）と、完成までの計画の節（`design/completion-plan`） | 設計と計画は merge 済み。実装は未着手 | `b974eb1`（`design/vad-end`）、`3f7369b`（`design/completion-plan`） | 2 本とも文書だけの branch。2 本を載せた木（`b3ecd1b`）の workspace の test は受付の実測で rc=0、results=115、passed=2694、failed=0、ignored=116（log は repo の外）。評議の GO は手順 1 と 2 まで | 方針 B の W の実装と E2E は、offline の門（H100）が通ってから。計画の節は 43〜246 行で、順位、残り、`L` の内訳、1000 ms の目標と 1600 ms の線の完成の日付、方針 C の 5 段を持つ。英語版に同じ節を足した | offline の門が未実行（L2）。計画の P と O の行は 2 つの設計が入る前の記録で数えてあり、数は設計の席の返答で直す（PL-H7） |
| VAD の trace（`feat/vad-trace`）と影の印（`feat/held-tail-shadow`） | 検証済み。影の印は VAD の設計（`rust_port.ja.md` の 3.17）の手順 1 に当たり、手順 2（offline の門）が次に出せる | `bdacc99`（`feat/vad-trace`）、`e5fdccd`（`feat/held-tail-shadow`） | 2 本とも可（検証の席の申告、git の外）。2 本を載せた木（`a22bb28`）の workspace の test と clippy は受付の実測で、test が rc=0、results=116、passed=2719、failed=0、ignored=117、clippy が rc=0 で警告と error が 0 行（log は repo の外）。`vad-trace` だけの木は 116／2704／0／117 で、差の passed +15 は影の印の test の分 | 影の印は判定を変えない。測る物で、`L` の p95 は縮めない（計画の節の表）。trace の記録は `docs/results/vad-trace-20261020/` | offline の門（H100）の 2 本の probe（`r8v_latency_set_probe_shadow_rule`、`r8v_heldout_ja_probe_shadow_rule`）が未実行 |
| E2E の境界の test（T-E2E-1-015、T-E2E-2-011） | merge 済み。書いていない席の検証は記録に無い | `8f3468b`（merge の commit は無く、fast-forward で入った） | `-p hayamimi-eval` の test は Running 31＋Doc-tests 1＝32、ok 32、FAILED 0。workspace 全体の test はその時は打っていない（`state.json` の記録） | E2E-1 は 70/80 で通り 69 で落ちる（`the_translated_share_passes_at_70_of_80_and_fails_at_69`）。E2E-2 の境界は 20/23（`the_translated_share_boundary_is_20_of_23`） | 書いていない席の検証 |
| R8v の設計（RP-66、RP-67、3.17）と VAD の閾値の probe | 設計は merge 済み。R8v の実装は未着手 | 設計 `27efb3c`。probe `42132a1` | 設計の merge は documents のみで test を打っていない（merge の commit の本文）。probe の表は `docs/results/vad-threshold-20261015/README.md` の 6 行と一致し、閾値は 0.5 のまま（`state.json` の記録） | RP-66 と RP-67 は未実装（冒頭の表） | 停止条件 2 に当たった offline の probe（`feat/r8v-probe`）は main に無い |
| RP-68 の test と H75 の直し（`Engine::finish` の後に detector を作り直す）、設計の直し（RP-68、H75、H76） | 検証済み | `5522942`（`5660032` と `4ca0f86` を含む） | 独立検証で可（merge の commit の本文）。workspace の test は Running 120＋Doc-tests 7＝127、ok 127、FAILED 0（`state.json` の記録、2 回目の run）。1 回目は `cs20` の `start_command_returns_while_the_server_keeps_running` が 24.8 s で落ちた（load 3.7 の時の時間の test で、差分とは無関係と記録されている）ので、式が合わず、同じ木で打ち直した | 直す前の main で `rp68` の 006、007、009、015 が落ち（子 process が SIGABRT）、直した後は通る（`5660032` の本文） | `cs20` の時間の test は load が高いと落ちる。RP-68 の子 3 は `finish` の後の feed を守るが、live の run で abort が起きた記録は探していない（H75） |
| 実機のマイクの確かめと、process A、B、中継の起こし方 | 検証済み | `5ad5da1`（`2d18f1d`） | 書いていない席の可（受付の申告、git の外）。merge は documents のみで test を打っていない（merge の commit の本文） | 2026-10-15 15:43 の run：`capture.fallback.none` 0 件、`capture.device.opened` 1 件（device は「リモート オーディオ」、kind は mic）、`asr.model.loaded` 1 件、`asr.final.published` 8 件。存在しない装置名の陰性対照は `capture.fallback.none` 1 件 | 人が日本語を話した結果は機械では確かめられない（冒頭の表） |
| 片づけの道具（`scripts/tidy-branches.sh` の `verify/` の規則） | 検証済み。`tidy-branches.sh` は、main と同じ位置の `verify/` の branch を、main-lock が空いているときに外す | `f0ba4f5`（merge の commit は `git log --merges -1 -- scripts/tidy-branches.sh` で引く） | 書いていない席の検証は記録に無い。merge の席（release-clerk）が merge の前に `tests/tidy_branches_verify.sh` を打ち直した | `tests/tidy_branches_verify.sh` が `cases=15 failures=0`。陰性対照は、lock を誰かが持つ、lock が古い、main-lock の binary が無い、自分の commit を持つ、worktree が持つ、`verify/` の外の branch の 6 つ | 対象は main と同じ位置の `verify/` の branch だけで、`verify/` の外の着手前の branch は今までどおり残す。lock が空かどうかは `main-lock check` の答えだけで読む |
| R11（Python を消す） | 入った。78 本と `requirements*.txt` を消し、CI を cargo に替えた | `docs/results/r11/ci_probe.md` | testdata と models の無い clone で CI の `run:` の行を打った結果が `ci_probe.md` に在る | RP-9、RP-14、RP-80、RP-81 | GitHub 上の最初の run は未確認（H103）。RP-19 の未実装の行は R13 が写す |
| 端から端の受入（E2E-1 マイクの ja→en、E2E-2 loopback の en→ja） | 単独の 1 run で受入 test が PASS。E2E-1 の正式な判定は RP-32 の交互の 3 対で出す。結果は冒頭の表の RP-32 の行 | `dfa85d6`（受入 test は `9103c95`） | 実装の席の申告（git の外）。設計の席は打ち直していない | E2E-1、E2E-2 の受入 test は PASS。数は上の E2E の行 | RP-32 の品質の差の原因。実機のマイクに人が話した ja は機械では確かめられない |
| Python を消す設計（`design/python-removal`、`rust_port.ja.md` の 3.2 と 8） | 設計と実装（R12a1、R12a2、R11c）が merge 済み。Python の file は 0 本で、CI は cargo だけ | `28990a2` | 文書だけの branch。木（`8b8041d`）の workspace の test は受付の実測で rc=0、suites 116、passed 2719、failed 0、ignored 117（log は repo の外）。評議の GO は `7dbf415`（RP-81 の入口の PAUSE を閉じた commit） | 消すのは R11 だけで、R11 に入る条件は 3 つ。組は R11a1、R11a2、R11b、R11c。rp19 と rp09 は R12a1、R12a2。置き換えの完了の定義と R11 の入口を引く 18 行（H110）は、R11c と同じ commit で書き替える | 作りかけの 7 つの tag は build も test も通していない（H102）。CI が GitHub で走るかは未確認（H103）。RP-14 の通過は status に記録が無い（H105） |
| R11 の前の文書の組のうち G0、R11a1、R11a2、R11b（`docs/r11-g0`、`docs/r11a1`、`docs/r11a2`、`docs/r11b`） | 4 本が merge 済み。R12a1 と R12a2 も次の行で merge 済み | `01ce0f7`、`49eef7f`、`e999000`、`9544240`（4 本を載せた木は `9e26fb3`、`1d6cae2`） | 文書と comment だけの branch。木の workspace の test と clippy は受付の実測で、test が rc=0、suites 121、passed 2836、failed 0、ignored 117、clippy が rc=0（log は repo の外）。前の suites 116、passed 2719 から増えたのは `hayamimi-ui` の 5 suite の 117 本（board 3、overlay_parity 8、rp12_pages 10、w7_limit 5、unittests 91）で、抜けた suite は 0 | G0 は RP-80 と RP-81 の case 行。R11a1 は README と文書の Python の command を Rust に替える。R11a2 と R11b は文書と Dart の comment | RP-14 の通過の記録が無い（H105） |
| RP-19 の照合（`feat/r12a1`）、RP-9 の確かめ（`feat/r12a2`）、Python の file を消す（`feat/r11c`） | 3 本が merge 済み。Python の file は 0 本（`git ls-files \| grep -c '\.py$'` が 0）。CI は cargo だけ | `b35076c`（R12a2）、`2de20fd`（R12a1）、`1967310`（R11c）。3 本を載せた木は `76e22ef`、`c9de8d5`。消す直前の main `e9799a3` に tag `archive/python-before-r11` | R12a2 と R12a1 は独立検証で可。R11c は不可の 1 点（`ci_probe.md` の数）を 1 行直し、受付が log と照らした。木の workspace の test と clippy は受付の実測で、test が rc=0、suites 125、passed 2924、failed 0、clippy（全部、`-D warnings`）が rc=0（log は repo の外）。`rp19_python_cases_are_ported` と `rp81_owned_exceptions_are_gone` は数を出して落ちる設計なので、CI と同じに `--skip` した | RP-19 の未実装の行は 172 行で、R13-1〜R13-13 が写す。`rp09.rs` は R12a2、`rp19.rs` と固定の一覧は R12a1 | GitHub 上の最初の CI の run は未確認（H103）。RP-19 の 172 行は未実装 |
| 重ね窓の設計（`design/overlay`）と OV-S1（`feat/ov-s1`） | 設計、OV-S1、OV-S2、OV-T、設計の差分 `130fbc9` が merge 済み（次の行）。OV-V（窓を開く受入）は未着手 | `4708711`（設計）、`59e2c32`（OV-S1）、`1d6cae2`（これらを載せた main） | 同じ木の test が上の数。OV-S1 は `hayamimi-ui` の `lanes.rs`（重ね窓の段の部品）と、その sheet が折り返さないことの検査 | 重ね窓として動くのは OV-V の受入が通ってから。CS-10〜CS-12 は未通過 | OV-V が未着手（持ち主は重ね窓の席） |
| RP-82 の設計（T の受入の test の名前が、実装の test と同じ）と R11c の後の直し（`design/r11c-followups`） | 設計が merge 済み。R13-15、R13-16、R13-17 を出せる。実装は未着手 | `d8a17f5`（merge の commit は無く、fast-forward で入った。評議 `a5d7272` の SCOPE_GO は `d8a17f5` に限る） | 文書だけの branch（`rust_port.ja.md` と `rust_port_tests.ja.md`）。文書を見張る test は受付の実測で、`d8a17f5` の detached の木の `-p hayamimi-eval --test rp80 --test rp81 -- --skip rp81_owned_exceptions_are_gone` が rc=0、rp80 は 19 passed、0 failed、2 ignored、rp81 は 31 passed、0 failed。workspace 全体の test は打っていない（文書だけの差分） | RP-82 は `rp82.rs` が `rust_port_tests.ja.md` の `test：` の行の名前を実装の test と照らす。落ちる行は G1 の前に 15 | `rp82.rs` は未作成。G1（T の `test：` の行の直し）は test-designer の席 |
| R13-18 の設計（GPU の無い機械で CPU へ落ちる受入を CI で走らせる。`design/rp16-ci-skip`） | 設計が merge 済み。実装は下の行で済み | `ebbf034`（merge の commit は無く、fast-forward で入った。評議 `a5d7272` の GO は `ebbf034` に限る） | 文書だけの branch（`rust_port.ja.md`）。文書を見張る test は受付の実測で、`ebbf034` の detached の木の `-p hayamimi-eval --test rp80 --test rp81 -- --skip rp81_owned_exceptions_are_gone` が rc=0、rp80 は 19 passed、0 failed、2 ignored、rp81 は 31 passed、0 failed | R13-18 は `native/hayamimi/tests/rp16.rs`、`.github/workflows/test.yml`、`docs/results/r11/ci_probe.md` を書く。CI の rp16 の 3 本の skip は R13-18 が外す。R13-18 の前に、test-designer の席が T-RP-16-015、049、050 と受入の行を直す（015 は評議の止めない点） | R13-18 は実装済み（下の行）。T の直しは test-designer の席 |
| R13-18 の T（T-RP-16-052〜062 の 11 本。`design/t-rp16-reason`） | merge 済み。R13-18 を出せる | `59c906c`（`157e8d4` を `--no-ff` で取り込んだ。評議 `a5d7272` の GO は `157e8d4` に限る） | 文書だけの branch（`rust_port.ja.md` と `rust_port_tests.ja.md`）。merge の差分は `157e8d4` の差分と patch-id が同じ。文書を見張る test は受付の実測で、`157e8d4` の detached の木の `-p hayamimi-eval --test rp80 --test rp81 -- --skip rp81_owned_exceptions_are_gone` が rc=0、rp80 は 19 passed、0 failed、2 ignored、rp81 は 31 passed、0 failed | R13-18 は起動の結果を `expected_reason` で受け、T-RP-16-052〜062 を持つ。062 は逆向きの陰性 | R13-18 は実装済み（下の行） |
| 重ね窓の S2 と T、設計の差分、CI の直し（`verify/ov3-76e6f40`、PR #2。`design/r11c-followups`、`fix/ci-first-run`、`feat/ov-s2`、`feat/ov-t`、`design/overlay-ov-s2-points`、`fix/ov-ci-rows` の 6 本） | merge 済み。GitHub の CI は ubuntu も windows も success | `2945489`（`76e6f40` を `--no-ff` で取り込んだ） | 受付の実測で、手元の test が rc=0、passed 3017、failed 0、clippy が rc=0（log は repo の外）。GitHub の CI の run 39415818064 は ubuntu も windows も success。merge 後の木と `76e6f40` の差は文書 6 本だけで、code と test は同じ | 重ね窓の crate `hayamimi-overlay` と `hayamimi-ui` の部品が main に在る。`test.yml` の `--test` は 102 語、`--skip` は 9 語 | OV-V（窓を開く受入）が未着手。R13 の slice が残る |
| R13-17 の受入 test（`feat/r13-17`。`rp32_validity_line_follows_the_rule_that_ended_each_final`） | merge 済み。T の `（未実装）` の印は 2 本になった（T-RP-10-007 は持ち主 R13-3、T-RP-16-037 は持ち主 R13-16） | `dd3d065`（`b49afc7` を `--no-ff` で取り込んだ） | 独立検証で可（検証の席の申告、git の外）：`test result: ok. 66 passed; 0 failed; 1 ignored`、標本の数を `jq` で数え直して 294 と 210 で一致、陰性対照 3 つが落ち、戻した後の sha256 は同じ。検証の木は `3bf4cfc`（tree `75d16b2`）。merge 後の木と `3bf4cfc` の差は文書 6 本だけで、`native/` と `.github/` は 0 行。文書を見張る test は受付の実測で、`-p hayamimi-eval --test rp80 --test rp81 -- --skip rp81_owned_exceptions_are_gone` が rc=0、rp80 は 19 passed、0 failed、2 ignored、rp81 は 31 passed、0 failed | `native/hayamimi-eval/tests/rp32.rs` に、各 `final` を確定した規則に従う有効の行の test を足し、T の 1267 行目付近の `（未実装）` の印を外した | R13-3 と R13-16 が未実装。workspace 全体の test はこの merge の後には打っていない |
| R13-18 の実装（`feat/r13-18`。CPU へ落ちる受入を CI で走らせる） | merge 済み。R13-18 は済み。GitHub の CI（run 39429756564 の再実行）は ubuntu も windows も success | `1d0dc90`（`f2e8317` を `--no-ff` で取り込んだ） | 独立検証で可（検証の席の申告、git の外。検証の木 `dc2b385`、tree `69053089`）。merge 後の木と `dc2b385` の差は `native/` と `.github/` が 0 行で、文書だけが違う。文書を見張る test は受付の実測で、`-p hayamimi-eval --test rp80 --test rp81 -- --skip rp81_owned_exceptions_are_gone` が rc=0、rp80 は 19 passed、0 failed、2 ignored、rp81 は 31 passed、0 failed | `rp16.rs` が期待の `reason` を自分で決める。`test.yml` の rp16 の 3 本の `--skip` と `ci_probe.md` の 3 本の行を外した。`ci_probe.md` に、runner の期待値は `driver_unreadable` と導いた値で、runner の `nvidia-smi` は NOT VERIFIED と書いてある | T-RP-16-037 の `（未実装）` は R13-16 の分として残る。workspace 全体の test はこの merge の後には打っていない |
| tidy が検証の木を外せるようになった（`fix/tidy-verify-trees`） | merge 済み | `718ce89`（`2cc9025` を `--no-ff` で取り込んだ） | 独立検証で可（検証の席の申告、git の外）。受付の実測は `tests/tidy_branches_verify_trees.sh` が cases=11 failures=0、`tests/tidy_branches_verify.sh` が cases=15 failures=0（sudo の 1 case は main-lock を持っていたため skip）。`native/` と `.github/` は 0 件 | `scripts/tidy-branches.sh` が、親が全部 main に在る検証の木を外せる | 親が main に無い検証の木と、commit 0 件の detached の木は拒む |
| tidy が、前の検証の merge の上に積んだ検証の木も外せるようになった（`fix/tidy-tree-equal`） | 済み。merge 済み | `1ecdc06`（`7dcc6df` を `--no-ff` で取り込んだ） | 独立検証で可（検証の席の申告、git の外）。受付の実測は `tests/tidy_branches_verify_trees.sh` が cases=23 failures=0、`tests/tidy_branches_verify.sh` が cases=15 failures=0（sudo の 1 case は main-lock を持っていたため skip）。`native/` と `.github/` は 0 件 | 外す条件は、tip を main に merge しても main の木が変わらないこと。main 側で revert した変更や、`-s ours` で取り込んだ変更も、この形になるので外す側に入る。外した後に失われるのは tip の merge commit だけで、gc が走るまでは戻せる | 衝突する merge と、手で解決した内容だけを持つ merge は残す |
| RP-83〜87 の T（`design/t-rp83-86`） | merge 済み。T が入った | `ce30fd2`（`12c0f62` を `--no-ff` で取り込んだ。評議の GO は `12c0f62` に限る） | 文書だけの branch（`rust_port.ja.md` と `rust_port_tests.ja.md`）。`native/` と `.github/` は 0 件。文書を見張る test は受付の実測で、`-p hayamimi-eval --test rp80 --test rp81 -- --skip rp81_owned_exceptions_are_gone` が rc=0、rp80 は 19 passed、0 failed、2 ignored、rp81 は 31 passed、0 failed。rp82 は `native/hayamimi-eval/tests/rp82.rs` がまだ無く（R13-15 の仕事）、打てていない | 今出す slice は R8x-1、R8x-3、R8x-4、R8x-2 の `rp67.rs` だけ。止める物は `rp79.rs`、L3、R8x-5、R8x-6、窓 E2、窓 E3 | 方針 B の offline の門（H100）は落ちた。B の規則を直して打ち直すのは 1 回だけ（評議の決定）。完成の日付（10-26 の夜〜10-27 の未明）は B が通る前提なので、B の決着の後に出し直す |
| R8x-1 の実装（`feat/r8x-1`。E2E の run の門、RP-83） | 済み。merge 済み | `7c8e466`（`29c96a5` を `--no-ff` で取り込んだ。評議の GO は `29c96a5` に限る） | 独立検証は `d08ebbe` の木 `dfcb8c4` で可（検証の席の申告、git の外）。`29c96a5` は `test.yml` に `--test rp83` を 1 語足しただけで、`d08ebbe` との差は `.github/workflows/test.yml` の 1 本。受付の実測は `-p hayamimi-eval --lib --test rp80 --test rp81 --test rp83 --test rp32 --test rp33 -- --skip rp81_owned_exceptions_are_gone` が rc=0。lib は 470 passed、rp32 は 67 passed・1 ignored、rp33 は 15 passed・1 ignored、rp80 は 19 passed・2 ignored、rp81 は 31 passed、rp83 は 54 passed、どれも 0 failed。初めの試みは、`test.yml` に rp83 が無く rp80 が落ちたため、merge せずに戻した | `hayamimi-eval stream` が `<run>.machine.jsonl` を書き、E2E の run の門が前後の load と run の間の cargo と rustc を判定する | workspace 全体の test と CI の run は、この merge の後には打っていない。`ci_probe.md` に rp83 の行は無い |
| `rp19.rs` が持ち主の札を最初の `.` より前で読む（`feat/rp19-names`）。R13 の slice は `<持ち主>.<slice>.txt` の名前で ported file を置ける | 済み。merge 済み | `656a66e`（`6bffabb` を `--no-ff` で取り込んだ） | 独立検証で可（検証の席の申告、git の外。検証の木は `6bffabb`）。merge の席は cargo を打っていない。main は基の `4deb57c` の後に `native/` と `.github/` を変えておらず（`git diff --stat 4deb57c a5cca72 -- native .github` が 0 行）、`rp19.rs` が読む T の 5.3 の表も `4deb57c` と `a5cca72` で同じなので、検証の席の結果が merge の後の木に当たる | 検証の席の申告：`cargo test --release -p hayamimi-eval --test rp19 -j 2 -- --skip rp19_python_cases_are_ported` が `test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out`。`rp19_python_cases_are_ported` は `172 rows of the table are not in the list yet` で落ちる（CI は skip）。陰性対照 2 本が落ち、戻した後の `git diff` は 0 行。clippy `-D warnings` と rustfmt は rc=0。差分は `native/hayamimi-eval/tests/rp19.rs` と `fixtures/rp19_r11/` の 3 本（`fixed.tsv`、`fixed_changed_slice.tsv`、`fixed_with_delta.tsv`） | R13-6・10・12 は下の行。R13 の残りの slice は受付が出す。workspace 全体の test はこの merge の後には打っていない |
| R13-6（ITN の test。`feat/r13-6`）、R13-10（`hayamimi config dump` と `ja_pipeline_spec`。`feat/r13-10`）、R13-12（話者の束ね直し。`feat/r13-12`） | 済み。merge 済み | `03cfdf3`（`dbbd76a` を `--no-ff` で取り込んだ）、`bfe7656`（`736faeb` を `--no-ff` で取り込んだ）、`4157f9b`（`e87b6c7` を `--no-ff` で取り込んだ） | 3 本とも独立検証で可（検証の席の申告、git の外）。検証の席は main の R8x-7 以降を通さずに打っているので、merge の席が merge の後の木 `4157f9b` で打ち直した。3 本が変えた file は互いに重ならず（`LC_ALL=C comm -12` が 0 行）、`git merge-tree --write-tree` は 3 本とも rc=0 | merge の席の実測（検証用の木を `4157f9b` に置き、CUDA 版の sherpa の lib で、`cargo-slot` を通して打った）：`-p hayamimi-eval --test rp19 --test rp80 --test rp81 --no-fail-fast -- --skip rp81_owned_exceptions_are_gone` は rp19 が `30 passed; 1 failed`、rp80 が `28 passed; 2 ignored`、rp81 が `31 passed; 1 filtered out`。落ちたのは `rp19_python_cases_are_ported` の 1 本だけで、`151 rows of the table are not in the list yet`（172 から R13-6 の 14、R13-10 の 3、R13-12 の 4 を引いた数。CI は skip）。`-p hayamimi-asr --lib` は `445 passed`（`pipeline::tests` の 5 本を含む）。`-p hayamimi --lib --bins --test ja_pipeline_spec --test rp84_start` は lib が `331 passed`、`ja_pipeline_spec` が `3 passed`、`rp84_start` が `7 passed; 1 ignored`。`-p hayamimi-eval --test rp83 --test rp84 --test rp67` は rp67 が `18 passed; 1 ignored`、rp83 が `90 passed`、rp84 が `36 passed; 1 ignored`。`cargo clippy --workspace --all-targets -- -D warnings` と `cargo fmt --all --check` は rc=0。検証の席の申告：R13-6 は `--lib itn` が `32 passed`、R13-10 は `--test ja_pipeline_spec` が `3 passed`、R13-12 は `--lib recluster` が `4 passed` | R13 の残りの slice は受付が出す。`rp19_python_cases_are_ported` は残りの 151 行が空になるまで落ちる。`cargo test --workspace` の全体は打っていない（打ったのは上の組、clippy、fmt） |
| README の冒頭の節を今の状態に書き直し（`docs/readme-current`）と、R13-15 の前提の数え直し・T の直し（`design/r13-15-counts`） | 済み。merge 済み | `40661a7`（`34f7515` を `--no-ff` で取り込んだ）、`298ef46`（`4e15b72` を `--no-ff` で取り込んだ） | 評議 2026-10-21 が 2 本とも可（評議の申告、git の外。可は `34f7515` と `4e15b72` に限る）。変えた file は互いに重ならず（`LC_ALL=C comm -12` が 0 行）、`native/` の差は 0 件、`git merge-tree --write-tree` は 2 本とも rc=0 | merge の席の実測（検証用の木を `298ef46` に置き、CUDA 版の sherpa の lib で、`cargo-slot` を通して打った）：`-p hayamimi-eval --test rp80 --test rp81 --no-fail-fast -- --skip rp81_owned_exceptions_are_gone` は rp80 が `28 passed; 0 failed; 2 ignored`、rp81 が `31 passed; 0 failed; 1 filtered out`。rp19 は cargo で打たず、入力が動かないことを確かめた：`git diff --stat 27218aa HEAD -- native/` が 0 行、T の差の hunk は 2 つだけで、どちらも `### 5.3` の対応表より前 | R13-15 の実装は rp82 の出力が 118 本・落ちる 0 と合うかを照らす |
| R13-1（命令と server の 18 行を Rust の test に写す。`feat/r13-1`）、R13-5a（音声の 9 行と `History::clear`。`feat/r13-5a`） | 済み。merge 済み | `efa39ca`（`09787bb` を `--no-ff` で取り込んだ）、`3a3009f`（`49bb982` を `--no-ff` で取り込んだ） | 2 本とも独立検証で可（検証の席の申告、git の外。可は `09787bb` と `49bb982` に限る）。変えた file は互いに重ならず（`LC_ALL=C comm -12` が 0 行）、`git merge-tree --write-tree` は 2 本とも rc=0 | 検証の席の申告：R13-1 は `-p hayamimi --lib` が `343 passed`、`--test r12b_routes` が `6 passed`、陰性対照 8 個が落ち、戻した後の diff は 0 行。R13-5a は `-p hayamimi-asr --lib` が `454 passed`、陰性対照 7 個が落ちた。2 本とも clippy と rustfmt は rc=0。`get_config_reports_current_values` と `post_config_invalid_value_returns_400_with_message` の期待値は Python と違い、main に元から在る挙動を固定した物で、評議 2026-10-21 が設計の 1 行より先の merge を可とした。merge の後の `rp19_python_cases_are_ported` の残りは 124 行の見込み（151 から R13-1 の 18 と R13-5a の 9 を引いた数。内訳の見込みは R12b 11、R12c 21、R12f 5、R12g 15、R12h 0）で、merge の席が merge の後の木 `d452186` で打った：rp19 は `rp19_python_cases_are_ported` の 1 本だけが `124 rows of the table are not in the list yet` で落ち、rp80 は `28 passed; 0 failed; 2 ignored`、rp81 は `31 passed; 0 failed; 1 filtered out`、`-p hayamimi --lib` は `343 passed`、`--test r12b_routes` は `6 passed`、`-p hayamimi-asr --lib` は `454 passed`、workspace の clippy と fmt は rc=0。最初の merge の後の木では rp80 が、`r12b_routes` が `test.yml` の `--test` の一覧に無いために落ちたので、一覧に足した（`0a9be8c`。空白が 1 つ欠けた分を `d452186` で直した） | `History::clear` の `:2815` の文言、`live_path_preroll_*` の model つきの経路の持ち主、T の `stream.rs` を `audio.rs` に直すか |
| R13-2（VAD の 8 行を Rust の test に写し、`Vad::rebuild` を直す。`feat/r13-2`）と、`/config` の持たない member の返し方を決める設計の 1 行（`design/config-values`） | 済み。merge 済み | `abde91f`（`bf4d386` を `--no-ff` で取り込んだ）、`f951ef0`（`f5c0139` を `--no-ff` で取り込んだ）。`test.yml` に skip を足した `ci:` の commit は `e444fee` | R13-2 は独立検証で可（検証の席の申告、git の外。可は `bf4d386` に限り、検証の木は `135a2d5`）。設計の 1 行は評議 2026-10-21 が可（評議の申告、git の外。可は `f5c0139` に限る）。`git merge-tree --write-tree` は 2 本とも rc=0 | 検証の席の申告：`-p hayamimi-asr --lib` が `457 passed`、`--test r12b_live_vad` が `6 passed`、rp80 が `28 passed; 0 failed; 2 ignored`、rp81 が `31 passed; 0 failed; 1 filtered out`、workspace の clippy と fmt は rc=0。merge の後の `rp19_python_cases_are_ported` の残りは 116 行の見込み（124 から R13-2 の 8 を引いた数。内訳の見込みは R12b 5、R12c 20、R12d 19、R12f 5、R12g 15、R12i 4、R12q 2、R12r 4、R12t1 9、R12t2 5、R12t3 7、R12u2 4、R12w 17、R12v 0）。`-p hayamimi-asr --lib` の 3 本（`live_vad_failed_rebuild_preserves_active_and_pending_state`、`live_vad_empty_update_does_not_rebuild_detector`、`live_vad_failed_early_rebuild_keeps_the_old_detector`）は `models/silero_vad.onnx` を読むので、CI の clone では落ちる。`test.yml` に `--skip` を 3 本足し、`ci_probe.md` に名前と理由を書いた | `Vad::rebuild` は early の build の失敗でも古い detector を残す形に直した（`bf4d386`）。(a) `/config` の 3 member（`dual_confirm`、`lid_switch_confirm`、`min_switch_s`）を `Router` につなぐかは、完成の後の設計の席が決める。つなぐ時は R13-1 の 2 test と `rust_port.ja.md:703` の 1 行を直す。(b) 検証の席が挙げた弱い所 3 つ（評議 2026-10-21 が持ち主を決めた）：(1) build が threshold を使わない形でも全部の test が通る（`applies_pending` は `sensitivity()` の値だけを見て、`real_speech` は基準も `rebuild(0.7)` で作る）。(2) 保留中の request が失敗で残る側は `native/hayamimi/src/session.rs:310` の `?` が担い、R13-2 の test は測らない。(3) `live_vad_waits_for_completed_segment_to_be_drained` は、取り出した segment の中身と取り出した後の作り直しを見ない。持ち主は、(2) が R13-13 の席、(1) と (3) が test を強める小さな slice（R13-2b。出すかは受付が日程で決める） |
| R13-5b（session の reset と `final` の `switched` を Rust の test に写す。RP-19 の 10 行。`feat/r13-5b`） | 済み。merge 済み | `09f91c1`（`49fc1c2` を `--no-ff` で取り込んだ） | 独立検証で可（検証の席の申告、git の外。可は `49fc1c2` に限り、検証の木は `verify-r13-5b`）。R13-2 の merge と変えた file は重ならず、`git merge-tree --write-tree` は rc=0 | 検証の席の申告（`4eeff80` の木）：`-p hayamimi --lib` が `354 passed`、`-p hayamimi-asr --lib` が `456 passed`、rp80 が `28 passed`、rp81 が `31 passed`、`--test r8a_relay` が `1 passed`、`--test rp12` が `15 passed; 1 ignored`、workspace の clippy と fmt は rc=0。新しく足した test は `models/` も `testdata/` も読まない。merge の後の `rp19_python_cases_are_ported` の残りは 106 行の見込み（116 から R13-5b の 10 を引いた数。内訳の見込みは R12b 1、R12c 18、R12d 19、R12f 5、R12g 15、R12i 0、R12q 2、R12r 4、R12t1 9、R12t2 5、R12t3 7、R12u2 4、R12w 17）。`-p hayamimi-asr --lib` は R13-2 の 3 本を足して `459 passed` の見込み | 検証の席が挙げた弱い所 4 つ（評議 2026-10-21 が持ち主を決めた）：(1) `Session::reset` から `Engine::reset_session` を通って `reset_parts` へ届く本番の経路を見る test が無い。陰性対照 (d)(d2)(g) で落ちる test が 0 で、Python の `reset_calls == ["asr"]` に当たる物が無い。持ち主は R13-13 の席（`session.rs` を書く）。(2) `final_event_carries_audio_s_lid_ms_decode_ms` は wire の半分だけを見て、`audio_seconds` を segment の長さから作る所（`pipeline.rs`）は見ない。(3) rp12 は `switched` の値を見ない（key と型だけ。値は lib の test が見る）。(4) `session_stats_reset_clears_last_final_lang` は `Stats::default()` を直に使い、本番の reset 経路は `reset_live_session_emits_summary_then_reset_in_order` が守る。(2)(3)(4) の持ち主は、R13-2 の弱い所 (1)(3) と一緒に R13-2b（弱い test の強化、test だけ。R13-13 が走る間の空いた cargo の枠で出す前提） |

作業中の branch は 1 つある。
`feat/r8v-probe` は R8v の手番 1 の probe で、流し直しを待っている。

## 採らない手段

| 採らない手段 | 理由 |
|---|---|
| 進み具合を `state.json` だけに置く | git の外に在り公開の repo に載らないので、読み手がどの slice が済んだかを見られない |
| `rust_port.ja.md` の slice の表に状態の列を足す | merge のたびに設計が動き、「通った」を定める所と通った記録が混ざる |
| 行番号で指す | 直すたびに動くので、slice と commit の名前で指す |
