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
| R3a（言語の振り分け。`hayamimi-asr` の `route` と `lid`） | 実装済み・検証待ち（branch `feat/r3-lang`） | なし | なし。実装した席の自分の run だけ | `route.rs` と `lid.rs` の RP-19 の名前 79 本が、台本の認識器と言語判定に対する単体 test として pass。RP-16 の言語判定の行は、`rp16.rs` の fixture T-RP-16-018〜022 と、`hayamimi-asr/tests/rp16_lid.rs` の model を使う実走 T-RP-16-023〜026（CPU、GPU、model が無いとき）。`hayamimi-asr/tests/gpu_guard.rs` の T-RP-16-027〜030（device を隠して `Gpu::Try` で開くと、言語判定と句読点が CPU に落ちて process が生きる。device の数を数えない run は終わる）。`lid_dual.rs` の T-RP-16-031〜034（golden の 8 本で、whisper-tiny 単独は 6 本が ja、二重確認の後に残る言語は 8 本とも ja）。RP-29 の R3 の行は fixture だけ（T-RP-29-022〜026）。RP-1 は未実行（R3b の `scorecard` が要る）。RP-29 の R3 の行の実走は無い（`hayamimi` の binary がまだ振り分けない） | `Decode` trait は R3b の merge まで台本で代用。`--lang auto` は binary を繋ぐまで使えない。ko の分かち書きとユーザーの辞書は振り分けの後ろに置く |
| R3b（認識器、常駐、`scorecard`、`lat-multi`） | 計画済み（W1） | なし | なし | RP-1、RP-22、RP-29 の R3 の行の実走は未実行 | H13、H24、H25。SenseVoice の言語の tag（`Decoded.tag`）が sherpa-onnx 1.13.8 の安全な結果に無い（`offline_asr.rs` の `get_result` は text、tokens、timestamps だけを読み、stream の pointer は `pub(crate)`）。`Decode` の実装は、library の JSON の `lang` を `sherpa-onnx-sys` で読む必要がある |
| R4（話者） | 実装済み・検証待ち | なし（branch `feat/r4-speaker`） | 書いた席の自分の run：`docs/results/r4/rp15_der.md`、`docs/results/r4/der_rust_run*.log`、`docs/results/gpu_stages.md` の clustering の行 | RP-15 の T-RP-15-001〜007 と 009〜011 が pass。RP-29 の R4 の場面は実走で pass。RP-8 の根拠 4 は成立（CPU の p50 24〜30 µs、CUDA の下限 222〜278 µs）。**T-RP-15-008 は不合格**：Rust の DER の平均 13.30、Python の平均 13.17（+0.13、Python の幅 0.20） | T-RP-15-008 は持ち主の読み（幅の内とみなすか、Python の run を増やすか）が要る。Python の幅の原因は未検証。話者の埋め込みが、字幕を出す thread で CPU の p50 を約 70 ms 足す |
| R5（取り込み、速報、server の全 route） | 計画済み（W1） | なし | なし | RP-10、RP-11、RP-12、RP-29 の R5 の行は未実行 | H34 |
| R6（Dioxus の 3 頁） | 計画済み（W2。designer の板は W1 の間） | なし | なし | RP-12 の頁の行は未実行 | designer の板が先 |
| R7（認識の遅れの詰め） | 計画済み（W2） | なし | なし | RP-4、RP-7 は未実行 | H9（0.25）、H10、H31 |
| R8a〜R8f（訳の中継の Rust の `hayamimi` への接続と訳の速度） | 計画済み（R8a は W1、R8b、R8c、R8d は W2、R8e、R8f は W3） | なし | なし | RP-3、RP-5、RP-13、RP-25、RP-28、会話字幕の CS-20〜CS-22、CS-25 は未実行 | H6、H17、H32、H8b |
| R10（取得、文書、英語版） | 計画済み（W3） | なし | なし | RP-14、RP-19 の spec の行、RP-21 は未実行 | H16 |
| R11（Python を消す） | 計画済み（W4） | なし | なし | RP-9、RP-19 は未実行 | 3.2 の 3 つの条件 |
| 端から端の受入（E2E-1 マイクの ja→en、E2E-2 loopback の en→ja） | 計画済み（W4 の後、05:45〜06:45） | なし | なし | E2E-1、E2E-2 は未実行 | 実機のマイクに人が話した ja は機械では確かめられない |

作業中の slice は、いまは無い。

## 採らない手段

| 採らない手段 | 理由 |
|---|---|
| 進み具合を `state.json` だけに置く | git の外に在り公開の repo に載らないので、読み手がどの slice が済んだかを見られない |
| `rust_port.ja.md` の slice の表に状態の列を足す | merge のたびに設計が動き、「通った」を定める所と通った記録が混ざる |
| 行番号で指す | 直すたびに動くので、slice と commit の名前で指す |
