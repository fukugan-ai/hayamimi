# hayamimi の実装の状態

正本は英語版の [status.md](status.md) で、この文書はその日本語版である。食い違ったときは英語版に従う。

slice ごとに、状態、merge の commit、検証の出所、受入の通過、残る穴を 1 行ずつ書く。
slice の中身と通す受入は `rust_port.ja.md` の 8 に、要件は [requirements.ja.md](requirements.ja.md) に在る。
commit の番号は `git log --oneline` で確かめた値で、確かめた時の main の先頭は `f0aeaf4` である。

状態の語は 5 つである。

- **済み**：merge され、書いていない席の検証が可を出した。
- **実装済み・検証待ち**：branch に commit してあり、merge はまだで、書いていない席の検証がまだ済んでいない。
- **作業中**：branch か worktree で作っている。
- **計画済み**：まだ作り始めていないが、07:00 の計画（`rust_port.ja.md` の 8 の「07:00 の計画」）の波に入っている。W1 は 2026-10-14 の 14:00〜20:00 JST、W2 は 21:00〜01:00、W3 は 2026-10-15 の 01:45〜04:15、W4 は 04:45〜05:45 である。
- **未着手**：まだ作り始めておらず、波にも入っていない。

| slice | 状態 | merge の commit | 検証の出所 | 受入の通過 | 残る穴 |
|---|---|---|---|---|---|
| 会話字幕 slice 1（`hayamimi-relay`） | 済み | `08cfdd4` | `rust_port.ja.md` の 8（「済んでいる」） | 会話字幕の設計の順 1〜13 | 会話字幕の設計 9 の残る穴 |
| R0（Python の基準と `hayamimi-eval`） | 済み | `5bb6343`、`228a88b` | `5bb6343` の commit の 1 行目（「verified (RB-1..8)」） | RB-1〜RB-8 | `rust_port.ja.md` の H14、H26 |
| R1（計測の slice） | 済み | `1b93167` | `1b93167` の commit の本文（RC-1〜RC-4 の出力） | RP-17、RP-24（`83af5a5` で verified）。RP-7、RP-8、RP-20、RP-23（Windows の `--ignored` で pass） | H29（負荷の分け方、訳 T の測り直し）、H30（fp16）、H31（R7） |
| R1b（評議と仮の値） | 済み | D `68867d8`、T `92f2641`、test `d921b21`、結果 `47341b4` | 評議の判定（git の外）を D に写した `68867d8` | 対の比較の 20 / 23 と根拠 3 の 1% を実測で裏づけ、`--min-silence` を 0.30 に決めた | H8（2000 ms、20%、14,000 MiB は据え置き） |
| R2（ja の経路、`--wav`、`/events`、CPU への fallback） | 済み | `f0aeaf4` | 独立検証の 2 回目で可（検証の席の返答、git の外）。直しは `a8840a7`、RP-6 の受入の置き直しは `1d33a5b` と `255c6a0` | `a8840a7` の本文の WSL の run で rp06 3、rp16 17、rp26 12、rp29 22、rp02 10、rp18 8、rp27 7 が pass。T-RP-6-012 は RP-6 の置き直しの後に通った | H8c（RP-6 の閾値）、H33（CPU に落ちた句読点の一致の件数）。T-RP-21-001、005、006、007 は R10 |
| R3a（言語の振り分け。`hayamimi-asr` の `route` と `lid`） | 済み | `7d82842` | 独立検証で可（検証の席の返答、git の外） | 通過は RP-16 の言語判定の行（T-RP-16-018〜026）と、RP-29 の R3 の行の fixture（T-RP-29-022〜026）。RP-1 は R3b を待つ。`route.rs` と `lid.rs` の RP-19 の名前 79 本が、台本の認識器と言語判定に対する単体 test として pass。RP-16 の言語判定の行は、`rp16.rs` の fixture T-RP-16-018〜022 と、`hayamimi-asr/tests/rp16_lid.rs` の model を使う実走 T-RP-16-023〜026（CPU、GPU、model が無いとき）。`hayamimi-asr/tests/gpu_guard.rs` の T-RP-16-027〜030（device を隠して `Gpu::Try` で開くと、言語判定と句読点が CPU に落ちて process が生きる。device の数を数えない run は終わる）。`lid_dual.rs` の T-RP-16-031〜034（golden の 8 本で、whisper-tiny 単独は 6 本が ja、二重確認の後に残る言語は 8 本とも ja）。RP-29 の R3 の行は fixture だけ（T-RP-29-022〜026）。RP-1 は未実行（R3b の `scorecard` が要る）。RP-29 の R3 の行の実走は無い（`hayamimi` の binary がまだ振り分けない） | `Decode` trait は R3b の merge まで台本で代用。言語判定はまだ `hayamimi` の binary に繋がっていない（`Engine::open` が `lid::open` を呼ばない。持ち主は R3b、T-RP-16-035）。CUDA と ORT の session を同じ process で同時に開くと abort することがある（並列 2 の 9 回で 2 回）ので、R3b が `hayamimi-gpu` に session を作る所の process 全体の lock を置く（T-RP-16-036、037）。`--lang auto` は binary を繋ぐまで使えない。ko の分かち書きとユーザーの辞書は振り分けの後ろに置く |
| R3b（認識器、常駐、`scorecard`、`lat-multi`、言語判定の結線、session の lock） | 作業中 | なし | なし | RP-1、RP-22、RP-29 の R3 の行の実走は未実行 | H13、H24、H25。SenseVoice の言語の tag（`Decoded.tag`）が sherpa-onnx 1.13.8 の安全な結果に無い（`offline_asr.rs` の `get_result` は text、tokens、timestamps だけを読み、stream の pointer は `pub(crate)`）。`Decode` の実装は、library の JSON の `lang` を `sherpa-onnx-sys` で読む必要がある |
| R0b（worktree から本体の `testdata/` を引く test の support） | 済み | `32ecb2a` | 独立検証で可（検証の席の返答、git の外） | `testdata/` の無い worktree で `hayamimi-eval` の test が FAILED 0。陰性対照は、探し先を両方とも無い path にすると、その 2 つの path を名指しして落ちる。symlink は 0 件 | 未実行：本体にも worktree にも `testdata/` の無い clone で落ちること（`rust_port.ja.md` の 8 の文言）。検証の席は、2 つ目の探し先を差し替える形で同じ落ち方を確かめた |
| R4（話者） | 済み | `ff85b01` | 独立検証で可、`48a21f0`（検証の席の返答、git の外） | `cargo test` は FAILED 0。`--include-ignored` の実走で rp15 が 13、rp15_speaker が 5、rp16 が 37、rp29 が 32 passed。T-RP-15-012 の順序の陰性対照あり。R5 の rp10、rp11、`--source` は通ったまま。clippy 0、漏れ 0 | `notice.rs` の `STAGES` と `REASONS` を `rust_port.ja.md` の 3.15 の一覧と突き合わせる test はまだ無い（持ち主は R10） |
| R5（取り込み、速報、server の全 route） | 済み | `2efbd5c` | 独立検証で可、`9a3f512` と `0767efb`（検証の席の返答、git の外） | `9a3f512`：rp10 が実走を含めて 13 passed（`lang` が `en` の条件は R3b を待つので除く）、`--source` が 2 passed、陰性対照あり。`0767efb`：`cargo test` が FAILED 0、rp11、rp29、Windows の build | NOT VERIFIED：Windows の実機での mic と loopback（E2E）。Linux の build に `libasound2-dev` が要る（`cpal` が `alsa-sys` を引く。`docs/guide/run_hayamimi.ja.md`） |
| R6（Dioxus の 3 頁、`/ui` の結線） | 済み | `7446f75` | 独立検証の席、画面テストの席、デザイナーの席（返答は git の外） | 検証：`cargo test` は FAILED 0 で 419 passed、W7 の陰性対照で 3 本が落ちた、clippy 0、衝突 0、漏れ 0。画面テスト：BD-1〜11 と W7 が通過、header は 60.80 px。BD-12 は通過（デザイナーの席。差 6 つは全部「許す」、「直す」は 0 件） | NOT VERIFIED：fallback の帯が在る時の DOM（帯の高さと `role="status"`）。E2E の画面の確かめで測る |
| R7（認識の遅れの詰め） | 計画済み（W2） | なし | なし | RP-4、RP-7 は未実行 | H9（0.25）、H10、H31 |
| R8a〜R8g（訳の中継の Rust の `hayamimi` への接続、訳の速度、外の送り先） | R8a: 検証済み（独立した席: CS-21 と CS-22 は陰性対照つきで通過。実音声 10 分の実行は E2E が受け持つ）。R8b: 検証済み。R8c: 検証済み。R8d: 検証済み。R8e、R8f、R8g（Zoom、Teams、SRT）: W3 で計画済み | R8a: `c868eb0`。R8b: `c663ce6`。R8c: `9278273`、chat の endpoint の直しは `022b283`。R8d: `57e4d6a` | R8a: 独立検証で可（検証の席の返答、git の外）。R8b: 独立検証で可、main の上で打ち直し（検証の席の返答、git の外）。R8c: 独立検証で可、統合の水準の陰性対照 2 本つき。chat の endpoint の直しも別の独立検証で可（検証の席の返答、git の外）。R8d: 独立検証で可、`e81451f`（検証の席の返答、git の外） | R8a: 会話字幕の CS-21 と CS-22 が偽の recognizer と陰性対照つきで通過。R8b: CS-20 の 001〜004、RP-16 の訳の行（T-RP-16-038〜050）、GPU 確認の陰性対照、既定の port 8080 が通過。main の上の `cargo test --release --no-fail-fast` は FAILED 0。R8c: CS-5、CS-25、RP-25 の 001〜009、RP-29 の sink の場面（偽の送り先）、T-CX-1-002 と 006 が通過。RP-25 の 010 は RP-3 の run（R8e）を待つ。R8c の直し：8080 の server に向けた probe1 の 40 行で、中継と curl の違いが 0 行、旧い `/completion` の形では 40 行とも違う。probe の log との一致は 40 行中 38 行。body を比べる test と陰性対照が通過。`timings.cache_n` を確かめる test はまだ無い。R8d: 977 passed、FAILED 0、陰性対照 2 本、clippy 0、衝突 0、漏れ 0。未実行は RP-3、RP-13、RP-28 | H6、H17、H32、H8b。R8b の未実行は T-RP-16-051（GPU のある機械で打つ）と CS-20 の 005（E2E の波が受け持つ）。R8d の未実行は T-RP-5-009 の実走（R8e）、`translate_partial` の打ち切りの実走（H6、R8e）、`show_partial` だけの陰性対照 |
| R10（取得、文書、英語版） | 計画済み（W3） | なし | なし | RP-19 の spec の行、RP-21 は未実行（RP-14 は R8f へ移した） | H16 |
| R11（Python を消す） | 計画済み（W4） | なし | なし | RP-9、RP-19 は未実行 | 3.2 の 3 つの条件 |
| 端から端の受入（E2E-1 マイクの ja→en、E2E-2 loopback の en→ja） | 計画済み（W4 の後、05:45〜06:45） | なし | なし | E2E-1、E2E-2 は未実行 | 実機のマイクに人が話した ja は機械では確かめられない |

作業中の slice は、いまは無い。

## 採らない手段

| 採らない手段 | 理由 |
|---|---|
| 進み具合を `state.json` だけに置く | git の外に在り公開の repo に載らないので、読み手がどの slice が済んだかを見られない |
| `rust_port.ja.md` の slice の表に状態の列を足す | merge のたびに設計が動き、「通った」を定める所と通った記録が混ざる |
| 行番号で指す | 直すたびに動くので、slice と commit の名前で指す |
