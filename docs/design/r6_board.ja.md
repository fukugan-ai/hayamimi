# R6 の板：字幕の 3 頁（overlay、dashboard、transcript）

`rust_port.ja.md` の R6（`hayamimi-ui`、Dioxus web）が写す板である。
3 頁の配置、token、状態ごとの見た目、event との対応、受入の測り方を置く。
設計の方針は `rust_port.ja.md` の 3.6、3.8、3.15 と `conversation_subtitles.ja.md` の 3.3、3.4 が決めており、この板はそれを描く形に落とすだけで、方針を変えない。

## 1. 読み方

| 頁 | URL | 誰が見るか | 描く物 |
|---|---|---|---|
| overlay | `/`、`/?show=final`、`/?show=partial` | 配信の視聴者（OBS の browser source） | 確定の行、速報の行、それぞれの訳、fallback の小さな印 |
| dashboard | `/dashboard` | 字幕を動かす人 | 速報の帯、確定のフィード、清書の列、fallback の帯 |
| transcript | `/transcript` | 清書を読む人 | 清書の段落と、その訳 |

最短の読み順は、2（作る順の判断）、3（token）、4（部品）、5〜7（頁）、9（event の対応表）、10（受入）である。
色と寸法は実装の席が写す値で、迷ったら 3 の表が正本である。

実装の席への条件は次の 4 つである。

1. overlay は 5 の条件で `mobile/hayamimi_core/lib/server/overlay_html.dart` と同じに描く（`tests/test_overlay_parity.py` が今それを測っている）。
2. 画面の文言は 11 の表から取り、`strings.<locale>.toml` に置く。code に直書きしない（設計 3.14）。
3. 頁は外の host へ 1 本も request を出さない（font の取得を含む）。
4. wire の key 名と値（速報の訳の `of: "partial"`、確定を知らせる `settled`、12 の W1 と W3 の仮の値）は、event を部品の状態へ変える 1 つの関数（adapter）に閉じ込める。key 名が決まったときに直す所をそこだけにするためである。

## 2. 作る順の判断

### 2.1 Figma の板について

Figma の台帳と板の URL は、この repo に無い（`rg -il figma` が当たるのは設計の md だけである）。
designer の席は Figma へ書く tool を持たないので、板の正本はこの文書である。
画像は置かない。
R6 が最初の 1 頁（dashboard）を 1440×900 で撮った画面と、この板を照らす（10 の BD-12）。

### 2.2 対象と証拠

| 項目 | 内容 | 出所 |
|---|---|---|
| 利用者 | 配信を動かす人と、その字幕を見る視聴者 | 設計 1.2、3.8 |
| 1 つの仕事 | 話した言葉が、遅れなく正しい言語の字幕と訳になって出ること（主指標は発話の終わりから訳の字幕までの遅れ） | 設計 3.1、3.6 |
| 語りの調子 | 速さが先に見えること。速報の帯を一番上に置き、確定の行と訳は読み順の中で暗い物から明るい物へ変わる（観察できる選び方は 3 と 4） | 設計 3.6 |
| 既存の画面の証拠 | 色、字、配置は `scripts/subtitle_server.py:83`（`DASHBOARD_HTML` の `:root` と CSS）、overlay は `:14-54`、transcript は `:57-80`。overlay の mobile 版は `overlay_html.dart:4-46` | 実読 |
| 無い物 | Figma の板、Dioxus の既存 component（`native/hayamimi-ui` は未作成） | `ls native` |

### 2.3 構造の 2 案

| 案 | 構造 | 判定 |
|---|---|---|
| A | 速報の帯を上に置き、確定のフィード（7）と清書の列（5）を並べる（今の dashboard の構造を残す） | 採る |
| B | 1 つの発話を 1 行にし、原文、訳、清書を横に並べる対訳の表 | 採らない |

B を採らない理由は、清書と確定の行が 1 対 1 でないことである。
清書は複数の確定文を段落にまとめ直した物で、2 つを結ぶ項目は無い（`conversation_subtitles.ja.md` の 3.3）。
B は結べない物を同じ行に並べることになる。

### 2.4 足した物と消した物

| 足した物 | 理由 |
|---|---|
| 速報の訳の行、確定の行の訳の行 | 設計 3.6 |
| fallback の帯と印 | 設計 3.15 |
| feed の「最新へ」の button | 利用者が過去の行を読んでいる間に、新しい行で画面が動かないようにする |
| 空の状態の 2 行 | 空の列が故障に見えるのを避ける |

| 消した物 | 理由 |
|---|---|
| dashboard の背景の放射状の明かりと走査線の模様 | 情報を持たない装飾。理由を記録できない飾りは残さない |
| 外部の font の取得（`fonts.googleapis.com`） | 外へ request を出さない。取得待ちで字が動かない |
| 「マイクの音声を待っています」の「マイク」 | loopback と `/ingest` でも同じ画面が出るため |
| 訳の「待ち」の行 | 訳が出ない言語（第三の言語、媒体の scene）では、待ちが消えないため |

### 2.5 製品の置き換えの検査と既定の形の棚卸し

「Realtime Translator」を他の製品名に替えても、次の 4 つは成り立たない。
言語ごとに色を持つ印、行ごとの遅れ（ms）、確定した部分と薄い部分を分けた訳、清書を話者ごとに段落で並べる列である。

| 既定の形 | 判定 |
|---|---|
| 暗い面 | 残す。今の画面の証拠で、OBS の重ね窓は透明で別物 |
| 大文字の字間つきの小見出し（`LIVE`、`REFINED`） | 残す。今の画面の声で、日本語の見出しと対で置く |
| card | 使わない。行は破線の区切りで並べる |
| gradient、glow、glass、画像 | 使わない |
| 点滅と脈動 | 残す。caret は聞き取り中、脈動は接続中を示す。どちらも `prefers-reduced-motion` で止める |

## 3. token

意匠の参照は Refero の Linear の entry（`https://styles.refero.design/style/90ce5883-bb24-4466-93f7-801cd617b0d1`）だけで、次の 3 つの寸法の考え方を採った。
4 px 刻みの間隔、2 / 4 / 6 px の角丸、12 / 13 / 15 px の小さい字の段である。
色と字体は採らない（Linear の黄緑の主色と Inter は他社の brand で、この製品の色は今の画面の物を残す）。

### 3.1 色

寸法の単位は px。色の名前は CSS variable 名（`--` を付ける）。

| 名前 | 値 | 使う所 |
|---|---|---|
| ink | `#0d0f13` | dashboard と transcript の地 |
| ink-2 | `#14171d` | 速報の帯、fallback の帯、chip の地 |
| line | `#2a2f3a` | 罫線、speaker chip の枠 |
| line-soft | `rgba(42,47,58,.7)` | feed の行の破線 |
| paper | `#ece5d8` | 本文、確定の訳 |
| paper-dim | `#b0a898` | 補助の字、先の訳、速報の訳の確定した部分 |
| paper-faint | `#8a8473` | 速報の訳の薄い部分 |
| shu | `#e04f2f` | 接続中の点、caret |
| c-ja | `#e04f2f` | 言語 ja の印 |
| c-en | `#5b7fd4` | en |
| c-zh | `#d4a13c` | zh |
| c-ko | `#3fae9d` | ko |
| c-yue | `#b06bc6` | yue |
| c-xx | `#7a8194` | 上の 5 つ以外と、言語が空の行 |

字の色の対比は 2026-10-14 に awk で WCAG の式から計算した（`(L1+0.05)/(L2+0.05)`、`L` は sRGB の相対輝度）。

| 前景 | 背景 | 対比 | 使う所 |
|---|---|---|---|
| paper | ink | 15.31 | 本文 |
| paper-dim | ink | 8.13 | 補助の字 |
| paper-dim | ink-2 | 7.61 | 帯の中の補助の字 |
| paper-faint | ink | 5.14 | 速報の訳の薄い部分 |
| paper-faint | ink-2 | 4.81 | 同上（帯の中） |
| ink | c-ja | 4.87 | 言語の印の字 |
| ink | c-en | 4.96 | 同上 |
| ink | c-zh | 8.19 | 同上 |
| ink | c-ko | 7.08 | 同上 |
| ink | c-yue | 5.27 | 同上 |
| ink | c-xx | 4.93 | 同上 |
| c-ja、c-en、c-zh、c-ko、c-yue、c-xx（字として） | ink-2 | 4.55、4.64、7.66、6.63、4.93、4.61 | 訳の行の矢印と言語の字 |

すべて 4.5 以上である。
overlay の白（`#fff`）は透ける面の上に置くので比は決まらず、`text-shadow` の黒い縁で読ませる（比は測らない。映像に依る）。

### 3.2 字

| 名前 | 値 |
|---|---|
| font-serif | `"Shippori Mincho B1", "Yu Mincho", "Hiragino Mincho ProN", "Noto Serif CJK JP", serif` |
| font-sans | `"Zen Kaku Gothic New", "Yu Gothic UI", "Meiryo", "Noto Sans CJK JP", sans-serif` |
| font-mono | `"IBM Plex Mono", "Cascadia Mono", Consolas, ui-monospace, monospace` |
| overlay の font | `"Yu Gothic UI", "Meiryo", sans-serif`（`overlay_html.dart:11` と同じ。変えない） |

font は端末に入っている物を使い、取得しない。
先頭の名前は入っていれば使われ、無ければ次へ落ちる（3.15 の「止めない」と同じ向き）。

| 段 | px | weight | line-height | 使う所 |
|---|---|---|---|---|
| fs-label | 12 | 400、600 | 1.4 | 小見出し、chip、遅れ、言語と話者の印、footer |
| fs-sub | 15 | 400 | 1.5 | 訳の行、補助の文 |
| fs-body | 16 | 400、600（清書） | 1.65（清書は 1.9） | 確定の行、清書 |
| fs-read | 18 | 600 | 1.9 | transcript の清書 |
| fs-strip | 24 | 600 | 1.5 | 速報の帯の原文 |
| fs-logo | 28 | 800 | 1.2 | logotype |

字間は `letter-spacing` で、小見出しが .35em、logotype が .12em、romaji が .3em、meta が .12em である。
12 px より小さい字は使わない。

### 3.3 間隔、角丸、影、動き

| 名前 | 値 |
|---|---|
| sp-1 から sp-10 | 4、8、12、16、20、24、32、40 |
| 頁の左右の余白 | 24（sp-6） |
| r-1、r-2、r-3 | 2（言語と話者の印）、4（chip）、6（button） |
| 影 | 使わない（overlay の `text-shadow` だけが例外で、5.2 の P3 の値） |
| 罫線 | 1 px、`--line` |

| 動き | 値 | 使う所 | 止める条件 |
|---|---|---|---|
| pulse | 1.6 s ease-out、infinite、0 から 10 px の輪が薄れる | 接続中の点 | `prefers-reduced-motion: reduce` |
| blink | 1 s steps(2)、infinite | caret | 同上（止めて表示のまま） |
| rise | 120 ms ease-out、4 px 下から | feed と清書の新しい行 | 同上（動かさず出す） |
| tone | 160 ms の `color` の遷移 | 先の訳が確定の訳に替わる所 | 同上（即時） |

行の出現を 120 ms にするのは、速さを主指標にする製品で、新しい行が見えるまでの時間を縮めるためである。

## 4. 部品

### 4.1 言語の印と話者の印

| 部品 | 見た目 | 状態 |
|---|---|---|
| 言語の印 | 12 px、weight 600、mono、字間 .08em、padding 2 × 6、r-1、地は 3.1 の言語の色、字は ink。字は `lang` の値そのもの（`ja`、`en` ほか） | `lang` が空なら地は c-xx で字は `??` |
| 話者の印 | 12 px、mono、padding 1 × 5、r-1、1 px の `--line` の枠、字は paper-dim、地は無し | `final` の `speaker` が空なら出さない。後から同じ `id` の `speaker` の event が届いたら、言語の印の後ろに印を足す。印は行の 1 行目に収まるので、行の数と高さは変えない。fade も動きも付けない |
| 遅れ | 12 px、mono、paper-dim。`round(latency_ms)` の後ろに `ms`。右端の列で右寄せ、幅は 5ch | `latency_ms` が `null` なら空（列は残す） |

### 4.2 訳の行（TranslationRow）

訳の行は、状態を 1 つ持つ enum（`Partial`、`First`、`Confirmed`、`Held`、`Unavailable`）で描く。
`Partial` の確定した部分の長さは、直前の速報の訳と今の速報の訳の最長共通接頭辞の長さ（Unicode の scalar value の数）である。
同じ長さを 3 回目以降の速報の訳へ持ち越さず、毎回「直前と今」だけで決める。
最初の速報の訳は直前が無いので、確定した部分は 0 で、全体が薄い。

| 状態 | dashboard の見た目 | overlay の見た目 |
|---|---|---|
| `Partial` | 確定した部分は paper-dim、残りは paper-faint。字形は直立（斜体にしない）。末尾に caret は付けない | 確定した部分は opacity 1、残りは opacity .5。字形は直立 |
| `First`（先の訳） | 全体を paper-dim、末尾に「先出し」の印（12 px、1 px の破線の枠、r-1、paper-dim） | 全体を白（opacity 1）。印は出さない |
| `Confirmed`（確定の訳） | 全体を paper。印なし。`First` から替わるときは文字列を置き換え、色を 160 ms で paper-dim から paper へ | `First` と同じ見た目で、文字列だけ置き換える |
| `Held` | 訳の文字列を出さず、「訳を控えました」の印と、`hold_reason` の語（11 の表）を paper-dim で出す | 行は空にする（相手へ何も送らない扱いに合わせる） |
| `Unavailable` | 「訳なし」の印を出す | 行は空にする |

速報の訳で字形を直立にそろえるのは、薄い部分が確定した部分へ移るときに、字の形が変わって見えないようにするためで、動くのは濃さだけである。
`First` から `Confirmed` への置き換えは同じ位置で 1 回だけ行い、行の数と高さを変えない（RP-25 の「置き換えは 1 回まで」を画面で守る）。

dashboard の行は、矢印と対象の言語を先頭に持つ。
矢印は `→` と訳の言語（`en` など）で、12 px の mono、字の色は 3.1 の言語の色である。
text は fs-sub である。

### 4.3 fallback の印

効いている fallback は、`stage` を key にした一覧で持つ。
`fallback` の event で `to` が文字列なら一覧を置き換え、`to` が `null` なら、その `stage` を一覧から消す。
遅れて接続した client が受ける、取っておいた event も同じ規則で畳む。

| 頁 | 見た目 |
|---|---|
| overlay | 画面の右上（`top: 2vh; right: 2vw`）に、「代替で動作中」と件数（2 件以上のときだけ数字）を 1 つの印で出す。字は 2.4vh、白、overlay と同じ `text-shadow`、0.15vh の破線の枠、角丸は .4em、padding は .3em .6em。一覧が空なら DOM に出さない。`?show=partial` では出さない |
| dashboard | header と速報の帯の間に、1 行の帯（ink-2、上下に `--line` の罫線）を出す。左に「代替で動作中」（12 px、paper-dim）、右へ 1 つの fallback につき chip を 1 つ並べて折り返す。chip は mono 12 px、1 px の破線の枠（`--line`）、r-2、字は `stage`、`→`、`to`、括弧で `reason`。一覧が空なら帯ごと出さない |
| transcript | 出さない |

色で伝えないのは、通常でないことを字と破線の形で伝え、言語の色（ja の shu ほか）との取り違えを避けるためである。
`stage`、`reason`、`to` の値は、文言の表（11）に載っていれば表の語、載っていなければ受けた文字列をそのまま出す。

## 5. overlay（`/`）

### 5.1 配置

```
┌──────────────────────────────────────────┐ 透明（alpha 0）
│                          [代替で動作中 2] │  top 2vh / right 2vw（fallback があるときだけ）
│                                          │
│            （映像が透ける）               │
│                                          │
│   確定の行        5vh   白、中央寄せ      │  #final-line
│   確定の訳        4vh   白               │  #final-tr   （訳の event を初めて受けてから）
│   速報の行        4vh   斜体、opacity .75 │  #partial-line
│   速報の訳        4vh   確定+薄い         │  #partial-tr （同上）
└─────────────────── bottom 4vh ───────────┘
```

`#box` は `position: absolute; left: 0; right: 0; bottom: 4vh`。
子は上から `#final-line`、`#final-tr`、`#partial-line`、`#partial-tr` の順に積み、すべて `display: block`。

### 5.2 overlay_html.dart と同じに描く条件

訳の event と fallback の event が 1 件も無い間、次の 7 つが `overlay_html.dart:4-46` と一致する。
訳の行と印は、この間 DOM に出さない。

| 番号 | 一致させる物 | 値（`overlay_html.dart` の行） |
|---|---|---|
| P1 | 要素の木 | `div#box` の中に `span#final-line` と `span#partial-line` の 2 つだけ（:18） |
| P2 | `html`、`body` | `margin: 0`、`background: transparent`、`overflow: hidden`（:8） |
| P3 | `#box` | `position: absolute; left: 0; right: 0; bottom: 4vh; text-align: center; font-family: "Yu Gothic UI", "Meiryo", sans-serif; font-size: 5vh; line-height: 1.4; color: #fff; text-shadow: 0 0 8px #000, 0 0 4px #000, 2px 2px 2px #000`（:9-14） |
| P4 | 2 つの行 | `display: block; min-height: 1.4em`。速報の行だけ `opacity: .75; font-style: italic; font-size: .8em`（:15-16） |
| P5 | `?show` | `final` なら速報の行を、`partial` なら確定の行を `display: none`。空と、それ以外の値は両方を出す（:23-27） |
| P6 | event の扱い | どの event も届くたびに 6000 ms の timer を掛け直す。`partial` は速報の行の text を置く。`final` は確定の行の text を置き、速報の行を空にする。timer が切れたら確定の行を空にする（:28-44） |
| P7 | 起動の見た目 | event が来るまで何も見えない（読み込みの印、点、枠を出さない） |

訳の行と fallback の印を足した後の振る舞いは次のとおりである。

- 訳の行は、`translation` の event を初めて受けたとき（`Held` と `Unavailable` を含む）に 2 行とも `display: block`、`min-height: 1.4em`（`.8em` の字）で出し、以後は消さない。初めの 1 回だけ確定の行が上へ動き、以後は動かない。
- 確定の訳の行は、確定の行と同じ timer で空にする。速報の訳の行は、速報の行と同じ時に空にする（`final` が届いたとき）。
- 訳の行の字は、`font-size: .8em`、`line-height: 1.4`、直立、`#box` から継ぐ `text-shadow`。
- 訳の event は、行の `id` に結ぶ（`final` の `id` と同じ `id` の訳は確定の訳の行へ、速報の訳は速報の訳の行へ）。
- `?show=final` は速報の行と速報の訳の行を、`?show=partial` は確定の行と確定の訳の行を隠す。fallback の印は `partial` で出さない。

`html`、`body` の透明は、頁全体（3 頁共通の CSS）が `html, body { margin: 0; background: transparent }` だけを持ち、dashboard と transcript は自分の根の要素で地を塗ることで守る。
共通の CSS に ink を置くと、overlay が OBS で黒くなる。

## 6. dashboard（`/dashboard`）

### 6.1 配置（1440×900）

```
┌─────────────────────────────────────────────────────────────────────┐
│ ● Realtime Translator               [確定 7] [平均応答 370 ms] [言語 en ko zh] │ header 約 61
├─────────────────────────────────────────────────────────────────────┤
│ 代替で動作中  [translate → cpu (unreachable)] [punct → cpu (no_device)] │ fallback の帯（あるときだけ）
├─────────────────────────────────────────────────────────────────────┤
│ いま聞き取り中                                                       │ 速報の帯
│ 它制作了一个个门铃▌                                                  │  fs-strip
│ →en  It made a door…（確定+薄い）                                    │  fs-sub（訳が出てから）
├──────────────────────────────────────┬──────────────────────────────┤
│ 確定フィード LIVE                     │ 清書 REFINED                 │ 7fr / 5fr
│ [en][S2] 原文……………………   827ms        │ S2 · en                     │
│          →en 確定の訳                 │ 清書の段落                   │
│ [ko][S3] 原文……………………   403ms        │ →ja 清書の訳                 │
│                          [最新へ]    │                              │
├──────────────────────────────────────┴──────────────────────────────┤
│ connected   /   /?show=final   /?show=partial   /transcript         │ footer
└─────────────────────────────────────────────────────────────────────┘
```

| 領域 | 寸法と見た目 |
|---|---|
| 根 | 高さ 100vh の縦の flex。地は ink、字は paper、`overflow: hidden`。背景の模様は無し |
| header | 高さは約 61 px（padding 12 × 2、28 px の logotype の行 33.6、下の罫線 1 に、baseline 揃えの字の余白が加わる。1440×900 の実測 60.8）。BD-12 の許容は 61 ± 2 px で、実装の padding は変えない。padding 12 × 24、下に罫線、`align-items: baseline`、gap 16。左から接続の点（10 px の円）、logotype（「早」は paper、「耳」は shu）、romaji（`hayamimi`、fs-label、字間 .3em、paper-dim）、空き、chip 3 つ |
| chip | fs-label、mono、1 px の `--line` の枠、r-2、padding 4 × 10、字は paper-dim。値は paper、weight 600。「確定」は確定の行の数、「平均応答」は `latency_ms` の平均の整数、「言語」は見た言語を `lang` の順に空白で並べる（値が無いときは `–`） |
| 接続の点 | 切れている間は c-xx。つながっている間は shu で pulse |
| 速報の帯 | 地は ink-2、下に罫線、padding 20 × 24 × 16、`min-height` 88。左上に小見出し「いま聞き取り中」（fs-label、字間 .35em、paper-dim）、原文は serif の fs-strip、小見出しの 16 下。原文の後ろに caret（幅 .5em、高さ 1.05em、shu）。訳の行は原文の 4 下 |
| 帯が静かなとき | 最後の `partial` から 4000 ms で、原文と訳の行に opacity .5 と paper-dim をかける（字は残す） |
| main | 2 列の grid（7fr と 5fr）、列の間は 1 px の罫線。それぞれの列は小見出しと scroll の領域 |
| 小見出し（h2） | fs-label、weight 700、字間 .35em、paper-dim、padding 12 × 24、下に罫線。右に `LIVE`、`REFINED`（shu） |
| scroll の領域 | padding 16 × 24 × 32、`overflow-y: auto`、細い scrollbar（`--line`） |
| 860 px 以下 | 1 列（確定、清書の順）。列の間の罫線は上の罫線に替える |

### 6.2 確定の行

`grid-template-columns: auto 1fr auto`、gap 12、padding 8 × 0、下に 1 px の `--line-soft` の破線、`align-items: baseline`。

| 列 | 中身 |
|---|---|
| 1 | 言語の印、話者の印（4.1） |
| 2 | 原文（fs-body、sans、weight 400）。その下に、同じ列で訳の行（4.2）。訳の行は gap 2 で、`grid-column: 2` |
| 3 | 遅れ（4.1） |

新しい行は rise（3.3）で出す。
訳は `id` で行に結ぶ（直前の行に貼らない）。
該当する `id` の行が無いときは、訳を捨てずに 5 秒だけ持ち、その間に行が届けば貼り、届かなければ捨てる。

### 6.3 清書の列

清書の段落は、meta の行（fs-label、mono、字間 .12em、paper-dim。話者があれば `S2 · `、後ろに言語を 3.1 の言語の色で）と、本文（serif、fs-body、weight 600、line-height 1.9）、本文の下の訳の行（4.2。`of` が `refine` の訳だけ）で組む。
段落の間は 16。
transcript と同じ部品（RefineParagraph）である。

### 6.4 追従と空の状態

| 状態 | 動き |
|---|---|
| 末尾から 48 px 以内にいる | 新しい行が来たら末尾へ追従する |
| 48 px より上を読んでいる | 追従しない。領域の右下に「最新へ」の button（fs-label、1 px の `--line` の枠、r-3、地は ink-2）を出し、押すと末尾へ動いて追従を戻す |
| 確定が 0 行 | 列の中央寄りに「まだ確定した行はありません」（fs-sub、paper-dim） |
| 清書が 0 行 | 「まだ清書はありません」 |
| 接続前と切断中 | 接続の点が c-xx、footer の接続の語が「接続中…」「再接続中…」。つながると「接続済み」 |

### 6.4.1 表示する行数の上限

確定のフィード、清書の列、transcript の段落は、どれも直近の 500 行（段落）だけを DOM に持つ。
500 を超えたら、古い行から DOM と model の両方で捨てる。
捨てた行は transcript へ寄せない。transcript は清書だけを持ち、確定の行とは結べないからである。

| 項目 | 決め |
|---|---|
| 上限 | 500 |
| 溢れた行 | 古い順に捨てる。1 行につき 1 行（追加と同時に先頭を捨てる） |
| 捨てた数の表示 | 列の先頭に「古い N 行は表示から外しました」（fs-label、paper-dim）を 1 行だけ出し、N を更新する。N が 0 の間は出さない |
| header の数 | 「確定」「平均応答」「言語」は捨てた行も含めた累計で出す（DOM の行数から数えない） |
| 追従していないとき | 先頭を捨てても、読んでいる位置を動かさない（捨てた行の高さだけ scroll を戻す） |

500 の根拠は、1 行を描く時間の実測が 1000 行まで 16 ms（1 frame）、2000 行で 23 ms、4000 行で 45 ms、8000 行で 93 ms、16000 行で 169 ms と行数に比例して伸び、50 ms の long task が 1000 行付近から出ることである。
速さを第一にするので、1 frame に収まる 1000 行の半分に置く。
行を描く時間が行数に比例するのは、model が行の増えるたびに全行を描き直すためと見える（NOT VERIFIED）。
描き直しを行ごとの差分にして上限を上げるかは、実装の席が測り、板へ戻す。

### 6.5 footer

fs-label、mono、paper-dim、padding 8 × 24、上に罫線、gap 24。
先頭に接続の語、続けて OBS へ貼る URL を `<a>` で 4 本（`/`、`/?show=final`、`/?show=partial`、`/transcript`）。
focus は `outline: 2px solid paper`、`outline-offset: 2px`。

### 6.6 読み上げ

確定のフィードと清書の列は `role="log"`、`aria-live="polite"`。
速報の帯は `aria-live="off"`（更新が多すぎる）。
fallback の帯は `role="status"`。

## 7. transcript（`/transcript`）

```
┌────────────────────────────────────┐ ink
│  S2 · en                           │ meta（fs-label、mono）
│  He would have to pay her the …    │ serif、fs-read
│  →ja 彼は彼女にお金を…              │ 訳（fs-sub、確定の訳は paper）
│                                    │
│  S3 · ko                           │
│  미국 공병대 시간당 …               │
└────────────────────────────────────┘
```

| 項目 | 値 |
|---|---|
| 根 | 地は ink、字は paper、padding 32 × 24、最小の高さ 100vh |
| 本文の幅 | `max-width: 720px`、左寄せ（画面幅が狭ければ全幅）。`margin-inline: 0` |
| 段落 | RefineParagraph（6.3）。本文の字だけ fs-read（18） |
| 追従 | 6.4 と同じ（末尾から 48 px 以内なら追従。「最新へ」を出す） |
| 空の状態 | 「まだ清書はありません」 |
| 読み上げ | 根に `role="log"`、`aria-live="polite"` |
| 出さない物 | 速報、確定の行、fallback の印、接続の表示 |

## 8. 速報の訳と先の訳の流れ（2 頁で共通）

1 つの発話の訳は、次の順に見た目が変わる。

```
partial の訳 ─┐ 受けるたびに全体を置き換える。確定した部分は「直前と今」の最長共通接頭辞
              │
final を受ける ─ 速報の訳の行を空にする（確定の訳の行は空のまま高さを保つ）
              │
T の訳（stage t、settled 偽） ── 確定の訳の行に `First`（先の訳）
              │
E の訳（stage e、settled 真） ── 同じ行で文字列を置き換えて `Confirmed`（置き換えは 1 回）
              │
E が V に落ちた（stage t、settled 真） ── 同じ文字列のまま `First` から `Confirmed` へ
```

`final` を受けた瞬間に速報の訳を確定の行へ持ち越さないのは、`final` の原文が最後の速報と違う場合に、別の原文の訳が新しい行の下に出るのを避けるためである。
その間の空きは、確定の訳の行が先の訳（T）で埋めるまでの短い時間で、行の高さは変わらない。

## 9. event と要素の対応表

型の名前は wire の値（`scripts/subtitle_server.py:148-157` ほか）。

| event の型 | overlay | dashboard | transcript |
|---|---|---|---|
| `partial`（`text`） | 速報の行 | 速報の帯の原文 | 使わない |
| `final`（`text`、`lang`、`speaker`、`latency_ms`、`id`） | 確定の行 | 確定の行、言語の印、話者の印、遅れ、header の 3 つの chip | 使わない |
| `translation`（`of` が `partial`） | 速報の訳の行 | 速報の帯の訳の行 | 使わない |
| `translation`（`of` が `final`、`id`、`stage`、`settled`、`status`、`hold_reason`、`lang`、`text`、`annotations`） | 確定の訳の行（`held` と `unavailable` は出さない） | 確定の行の訳の行 | 使わない |
| `translation`（`of` が `refine`、`id`） | 使わない | 清書の段落の訳の行 | 段落の訳の行 |
| `speaker`（`id`、`speaker`） | 使わない | 同じ `id` の確定の行の話者の印（4.1） | 使わない |
| `refine`（`text`、`lang`、`speaker`、`id`） | 使わない | 清書の列 | 清書の段落 |
| `fallback`（`stage`、`reason`、`to`） | 右上の印 | fallback の帯 | 使わない |
| `session_start`、`session_reset`、`session_summary`、`recluster`、`model_load`、`model_fallback`、`warning`、`error`、`ready` | 表示に使わない。届いたことだけが 6000 ms の timer を掛け直す | 表示に使わない | 表示に使わない |

`annotations` は、訳の `text` の中の Unicode の scalar の位置 `end` へ、その `text` を差し込んで描く。
差し込む字は、訳と同じ大きさで、dashboard と transcript では paper-dim にして換算の添え書きだと分かるようにする（`conversation_subtitles.ja.md` の 3.3 の差し込み位置の規則）。
overlay は、確定の訳の `annotations` も同じ位置に同じ見た目で差し込む。

`session_start` と `session_reset` で画面を消さないのは、接続し直した client が取っておいた `session_start` を再び受けるためで、消すと再接続のたびに画面が空になる。

## 10. 受入

実測は書いていない席が打つ。
測りは Playwright（1440×900 と、overlay は 1920×1080、1280×720）、欄の「陰性対照」は、その test が落ちることを先に確かめる入力である。

| ID | 何を測るか | 期待値 | 陰性対照 |
|---|---|---|---|
| BD-1 | overlay の computed style | 5.2 の P2 から P4 の物を、`getComputedStyle` で `html`、`body`、`#box`、`#final-line`、`#partial-line` について全部読み、`overlay_html.dart` の文字列を静的に開いた頁と全一致 | `font-size` を 5vh から 5.1vh にした build で落ちる |
| BD-2 | overlay の振る舞い（P5、P6） | 同じ event の列（partial、final、6000 ms 待つ、partial だけ 5 本、`?show=final`、`?show=partial`、`?show=x`）で、2 つの行の text が毎回一致 | timer を 5000 ms にした build で落ちる |
| BD-3 | overlay の画素 | 訳と fallback の event が無い同じ列で、2 つの頁の screenshot の差が 0 画素 | 同じ頁を 2 回撮った差は 0（陽性）。`opacity: .75` を `.7` にして差が出る |
| BD-4 | overlay の透明 | event が無い間、`omitBackground` の screenshot の alpha が全画素 0（P7） | 背景を ink にした build で落ちる |
| BD-5 | 速報の訳の分け方 | 訳の列「予算は五百万」、「予算は五百万円を想定」、「予算は五百万円で」で、確定した部分が 0 文字、6 文字（`予算は五百万`）、7 文字（`予算は五百万円`）、残りが薄い（dashboard の色と overlay の opacity を読む） | 最初の物を最長共通接頭辞ではなく文頭 1 文字で分ける build で落ちる |
| BD-6 | 先の訳から確定の訳への置き換え | `First` の後に `Confirmed` を送り、訳の行の text が 1 回だけ変わり、行の数と高さが変わらず、dashboard の色が paper-dim から paper へ変わる | 置き換えで行を足す build で落ちる |
| BD-7 | `Held` と `Unavailable` | dashboard は印と理由の語、overlay は訳の行が空 | `Held` で overlay に原文を出す build で落ちる |
| BD-8 | fallback の印 | 1 件で overlay と dashboard に出る。`to` が `null` の同じ `stage` で両方から消える。`session_start` より先に 2 件を送った後に接続した client でも同じ（overlay は `?show=partial` で出ない） | 消す event を捨てる build で落ちる |
| BD-9 | 色の対比 | 3.1 の表の全組が 4.5 以上（計算を再実行して表と一致） | 組の 1 つを paper-dim から c-xx にした地で落ちる |
| BD-10 | 外への request | 3 頁を開いて、同じ origin 以外への request が 0 件 | 外の font を足した build で 1 件以上 |
| BD-11 | 動きの停止 | `prefers-reduced-motion: reduce` で、3.3 の 4 つの `animation` と `transition` の実効が `none` | 指定を外した build で落ちる |
| BD-12 | dashboard の板との照合 | R6 が 1440×900 で撮った dashboard を、この板の 6.1 の配置と 3 の token に照らして、designer の席が差を列挙する | 差が 0 件でないとき、直すか理由を書く |

BD-1 から BD-4 は、Python の `tests/test_overlay_parity.py` が R11 で消えた後の代わりでもある。
それまでは `tests/test_overlay_parity.py` が Python と Dart の一致を測り続け、BD-1 から BD-4 が Rust と Dart の一致を測る。

## 11. 画面の文言

`strings.ja.toml` と `strings.en.toml` に置く。
key は `ui.<頁>.<名前>` の形にする。

| key | ja | en |
|---|---|---|
| `ui.dashboard.listening` | いま聞き取り中 | Listening |
| `ui.dashboard.waiting` | 音声を待っています | Waiting for audio |
| `ui.dashboard.feed` | 確定フィード | Final feed |
| `ui.dashboard.refined` | 清書 | Refined |
| `ui.dashboard.chip_finals` | 確定 | Finals |
| `ui.dashboard.chip_latency` | 平均応答 | Mean latency |
| `ui.dashboard.chip_langs` | 言語 | Languages |
| `ui.dashboard.empty_feed` | まだ確定した行はありません | No final lines yet |
| `ui.common.empty_refined` | まだ清書はありません | No refined text yet |
| `ui.common.jump_latest` | 最新へ | Jump to latest |
| `ui.common.dropped` | 古い {n} 行は表示から外しました | {n} older lines are no longer shown |
| `ui.common.connecting` | 接続中… | Connecting… |
| `ui.common.reconnecting` | 再接続中… | Reconnecting… |
| `ui.common.connected` | 接続済み | Connected |
| `ui.tr.first` | 先出し | Early |
| `ui.tr.held` | 訳を控えました | Translation withheld |
| `ui.tr.unavailable` | 訳なし | No translation |
| `ui.tr.reason.number` | 数の不一致 | Numbers differ |
| `ui.tr.reason.negation` | 否定の不一致 | Negation differs |
| `ui.tr.reason.uncertainty` | 推量の不一致 | Hedging differs |
| `ui.tr.reason.proper_noun` | 固有名の不一致 | Proper noun differs |
| `ui.tr.reason.form` | 形の不一致 | Malformed output |
| `ui.tr.reason.unavailable` | 翻訳器に届かない | Translator unreachable |
| `ui.lanes.held_mark` | 原文 | Original |
| `ui.lanes.untranslated_mark` | 訳なし | No translation |
| `ui.fallback.badge` | 代替で動作中 | Running on fallback |

`stage`、`reason`、`to` の語は、決まった値の分だけ `ui.fallback.stage.<stage>` の形で足し、載っていない値は受けた文字列を出す。
`final` と `lang`、`speaker` の値は wire の値で、切り替えない（設計 3.14）。

## 13. 頁の 3 つの枠と境目（IP-S12、設計 10.13.11）

IP-S12 が写す見た目を決める。
置き場、境目の振る舞い、`localStorage` の形、受入の BR-19 から BR-25 と BR-32 から BR-34 は、設計 10.13.11 が決めており、この節は変えない。
IP-S12 の後は、6.1 の「main」の行（2 列の grid、7fr と 5fr）と「860 px 以下」の行を、この節が置き換える。
6.1 の他の行は変えない。

利用者は、字幕を動かす人と、同じ画面を読む会話の相手である。
1 つの仕事は、2 つの言語の発話が、どちらの側の言葉かを一目で読み分けられることである。
語りの調子と意匠の参照は 2 と 3 のままで、色と字体は今の画面の物を残す。
既存の画面の証拠は、`css.rs` の `.latest`（:64-66）、`.chip`（:141-143）、footer の focus の輪（:176）、860 px の線（:177-180）である。

13 から 15 の文言の表は、行頭を部品の名前にし、key を 2 列目に置く（部品、key、ja、en の 4 列）。
`tests/board.rs` の `board_words` は、11 の見出しから最初の「12.」までの、行頭が `` | `ui. `` の行を、11 の文言の表（26 件）として数える。
13 から 15 は 12 の前に在るので、行頭を `ui.` にすると CI の `--test board` が落ちる。
IP-S12 の席は、この 4 列の表を読む test を足し、`board_words` の終わりの印を、12 の見出しの字から 13 の見出しの字へ替える。

**最新情報**：WAI-ARIA APG の Window Splitter は、focus できる `separator` に `aria-valuenow`、`aria-valuemin`、`aria-valuemax`、主な枠を指す `aria-controls`、名前を持たせ、縦の線は左右の矢印、横の線は上下の矢印で動かし、Home と End を最小と最大に当てる（https://www.w3.org/WAI/ARIA/apg/patterns/windowsplitter/ 、取得 2026-10-29）。
WCAG 2.2 の 2.5.8 は、pointer の的を 24 × 24 CSS px 以上にし、それより小さい的は、24 px の円が他の的や小さい的の円に触れない時だけ通す（https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html 、取得 2026-10-29）。
同 1.4.11 は、部品と状態を見分けるのに要る見た目（focus の輪を含む）に、隣の色との 3 : 1 を求め、枠は他に見分ける手がかりが無い時だけ要る（https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html 、取得 2026-10-29）。
APG の switch は、状態が変わっても名前を変えないことを求める（https://www.w3.org/WAI/ARIA/apg/patterns/switch/ 、取得 2026-10-29）。
MDN は、`color-scheme` が native の form 部品と scrollbar の色を明暗に合わせると書き（https://developer.mozilla.org/en-US/docs/Web/CSS/color-scheme 、取得 2026-10-29）、`color-mix()` を 2023 年 5 月から広く使える機能と書く（https://developer.mozilla.org/en-US/docs/Web/CSS/color_value/color-mix 、取得 2026-10-29）。

### 13.1 配置と 2 案

| 案 | 構造 | 判定 |
|---|---|---|
| A | 操作の並びを 2 つの列の上に 1 本置く（入れ替え、側ごとの字体と大きさ、switch）。境目は 1 px の線と、中央の短い棒（つまみ）。行は両方の列で左寄せ | 採る |
| B | 操作を header の右端の「表示」1 つにまとめ、開閉にする。境目は線だけで、pointer を重ねると太くなる。右の列の行は右寄せにして、左右を鏡に写す | 採らない |

B を採らない理由は 3 つである。
開閉の中に隠すと、選んだ字体と大きさが、開くまで見えない。設定は、準備の時に両方の側を見比べて直す物なので、見えている方がよい。
線だけの境目は、pointer を重ねるまで動かせると分からない。key だけで操作する人は、細い線の上の focus を見つけにくい。
右寄せの行は左端がそろわず、折り返した原文と訳の行頭を探すことになる（13.4）。

製品名を替えると成り立たなくなるのは、話された言語で行を左右に分けて側ごとに字体と大きさを持つ形、組を変えた所を 2 つの言語の印で書く区切りの行（14）、合言葉を URL の fragment で受けて flag に登録する origin を示す頁（15）である。
既定の形の棚卸しは次のとおりである。

| 既定の形 | 判定 |
|---|---|
| 丸い knob の pill の switch | 形を変えて残す。track は r-2（4）、knob は r-1（2）の角で、2 / 4 / 6 の外の pill を足さない |
| 点 6 つの「⋮⋮」の grip | 使わない。短い棒 1 本（4 × 32） |
| 歯車、月、太陽の icon の button | 使わない。語（「暗い表示」「既定」）で出す。画像の icon を取らない（1 の 3） |
| 緑、黄、赤の 3 色の meter | 使わない（15.5）。1 色の長さだけで読ませる |
| 吹き出しで左右を分ける | 使わない。行は破線の区切り（2.5 の card を使わない決め） |
| 影、gradient、glow、画面を覆う dialog | 使わない。開閉の欄は文書の流れの中で開く |

1440 × 900 の配置は次のとおりである（左の列が英語の側、右の列が日本語の側の既定の向き）。

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ ● Realtime Translator   会話の言語 [en▾][ja▾][決める]  [確定 7] [言語 en ja]             │  header 約 61
├──────────────────────────────────────────────────────────────────────────────────────────┤
│ 代替で動作中  [translate → cpu (unreachable)]                                            │  fallback の帯（あるときだけ）
├──────────────────────────────────────────────────────────────────────────────────────────┤
│ いま聞き取り中 [静か]                                                                    │  Listening の欄
│ 它制作了一个个门铃▌                                                                      │  （2 つの列にまたがる）
│ →en  It made a door…                                                                     │
├──────────────────────────────────────────────────────────────────────────────────────────┤
│ [左右を入れ替える] en [標準▾][−]100%[+][既定] │ ja [標準▾][−]100%[+][既定]  [暗い表示 ▮] │  操作の並び（高さ 45）
├──────────────────────────────────────────────────────────────────────────────────────────┤
│ 確定フィード                                                                        LIVE │  小見出し（全幅）
├────────────────────────────────────────────┰─────────────────────────────────────────────┤
│ [en][S2] They said the budget …      827ms ┃                                             │  左の列
│          →ja 予算は …                      ┃ [ja][S1] はい、そうです。             403ms │  縦の境目
│ - - - - - - - - - - - - - - - - - - - -    ┃          →en Yes, that is …                 │  （1 本、scroll は 1 つ）
│ [en][S2] Could you send me …         611ms ┃ - - - - - - - - - - - - - - - - - -         │
│                                   [最新へ] ┃                                             │
┝━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┿━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┥  横の境目
│ 清書                                                                             REFINED │  小見出し（全幅）
│ S2 · en  He would have to pay her the full amount …                                      │  清書の欄（2 つの列にまたがる）
├──────────────────────────────────────────────────────────────────────────────────────────┤
│ 接続済み   /   /?show=final   /?show=partial   /transcript                               │  footer
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

860 px 以下は 1 列である。
縦の線と入れ替えの button は隠し、字体と大きさの口は開閉の欄にまとめる。
横の線と清書の欄は残し、覚えた割合（`rows`）で描く。

```
┌────────────────────────────────────────┐
│ ● Realtime Translator                  │  header（折り返す）
│ 会話の言語 [en▾][ja▾][決める]          │
│ [確定 7] [言語 en ja]                  │
├────────────────────────────────────────┤
│ いま聞き取り中 [静か]                  │  Listening
│ 它制作了一个个门铃▌                    │
├────────────────────────────────────────┤
│ [文字の設定 ⌄]            [暗い表示 ▮] │  操作の並び
├────────────────────────────────────────┤
│ 確定フィード                      LIVE │
│ [en][S2] They said …             827ms │  1 列、時刻の順
│          →ja 予算は …                  │
│ [ja][S1] はい、そうです。        403ms │
│                       [最新へ]         │
┝━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┥
│ 清書                           REFINED │  横の境目は残す
│ S2 · en  He would have to …            │
├────────────────────────────────────────┤
│ 接続済み  /  /?show=final  …           │  footer
└────────────────────────────────────────┘
```

| 領域 | 寸法と見た目 |
|---|---|
| header | 6.1 のまま（約 61）。`flex-wrap: wrap` と `row-gap: 4px` を足す（1440 では折り返さず、高さは変わらない）。`gap: 16px; padding: 12px 24px;` の字面は変えない（`css.rs` の test が見ている）。右の端の chip の前に、会話の言語の欄（14.1）が入る |
| fallback の帯、Listening の欄 | 6.1 のまま。Listening の欄は 2 つの列にまたがる全幅。静まった時の印は 13.5 |
| 操作の並び | 13.3。高さ 45（28 + 8 × 2 + 下の罫線 1）。地は無し（ink）。約 1030 px 未満では折り返し、高さは 81 |
| 確定フィードの見出し | 6.1 の小見出し（h2）のまま。2 つの列にまたがる 1 本で、右に `LIVE` |
| 上の列 | 縦の線で左右に分ける 1 つの scroll の領域（`role="log"`、`aria-live="polite"`）。padding 16 × 0 × 32（左右の余白は 13.4 の囲みが 24 ずつ持つ）。最小の高さ 160（見出しを含む） |
| 縦の境目 | 13.2。scroll の領域の中の格子の 1 px の列に置き、内容と一緒に scroll する。高さは内容の高さ（短い間は領域の高さ） |
| 横の境目 | 13.2。上の列と清書の欄の間の 1 px の行 |
| 清書の欄 | 小見出し「清書 REFINED」（6.1 の h2）と、6.3 の `RefineParagraph` の scroll の領域。全幅、padding 16 × 24 × 32。最小の高さ 120（見出しを含む） |
| footer | 6.5 のまま。2 行目に dashboard の送り先の案内（15.8）が入ることがある |

板の値で組んだ検証用の mock（repo の外）を、`chrome-headless-shell` の 1440 × 900 で開いて `getBoundingClientRect` を読んだ。
字は Meiryo UI で描いた。fallback の帯と速報の訳の行が出ている、縦が最も厳しい場面である。

| 領域 | 高さ（px） | 備考 |
|---|---|---|
| header | 60.8 | 会話の言語の欄（部品の高さ 28）を入れても、6.1 の約 61 から変わらない |
| fallback の帯 | 40.8 | あるときだけ |
| Listening の欄 | 134.3 | 速報の訳の行が出ている時 |
| 操作の並び | 45 | 1 行 |
| 上の列（見出し 41.8 を含む） | 379.8 | 既定の 65 : 35。scroll の領域は 338。1 段は約 68（原文と訳の 2 行） |
| 横の境目 | 1 | つかむ領域は 25（13.2） |
| 清書の欄（見出しを含む） | 204.5 | |
| footer | 33.8 | |

合計は 900.0 である。
実装した頁の値は BD-14 が取る。

### 13.2 境目の見た目

| 項目 | 縦の線（左右の列の境目） | 横の線（上の列と清書の欄の境目） |
|---|---|---|
| 要素（つかむ領域） | 幅 25、高さは scroll の領域の内容の高さ。格子の 1 px の列から、左右へ 12 px ずつはみ出す（`margin-inline: -12px`） | 幅は全幅、高さ 25。格子の 1 px の行から、上下へ 12 px ずつはみ出す（`margin-block: -12px`） |
| 見える線（擬似要素） | 幅 1、中央、`--line` | 高さ 1、中央、`--line` |
| つまみ（擬似要素） | 幅 4 × 高さ 32、r-1。scroll の領域の見える高さの中央に留める（`position: sticky`、`top: calc(50% - 16px)`） | 幅 32 × 高さ 4、r-1。横の中央 |
| cursor | `col-resize` | `row-resize` |
| pointer | `touch-action: none`。drag の間は `user-select: none` | 同じ |

縦と横は同じ部品で、向きだけが違う。
線の太さ、つまみの長さ、色、輪はそろえる。

| 状態 | 線 | つまみ | 輪 |
|---|---|---|---|
| 通常 | 幅 1、`--line` | `--paper-faint` | なし |
| hover | 幅 3、`--paper-faint` | `--paper-dim` | なし |
| `:focus-visible` | 通常と同じ | 通常と同じ | 要素の内側に `outline: 2px solid var(--paper); outline-offset: -2px` |
| drag 中（pointer capture 中） | 幅 3、`--paper` | `--paper` | なし（focus が付いていれば輪も付く） |

境目の要素そのものがつかむ領域で、見える線とつまみは擬似要素で描く。
要素は隣の囲みより前面に置く（`z-index`）。focus の輪は要素に付くので、`getComputedStyle` で読める（BD-15）。
線は中心に重ねて太らせ、並びの寸法を動かさない。
どの状態の変化も動かさず、即時に替える（3.3 の動きの表に足さない）。drag に遅れを入れないためである。
tab の順は DOM の順で、操作の並び、縦の線、横の線の順に進む。

つかむ領域を 25 px にするのは、WCAG 2.2 の 2.5.8（的は 24 × 24 CSS px 以上）を、24 px の円が他の的に触れない時だけ通す間隔の例外に頼らず、大きさで満たすためである。
重なる 12 px は、各列の囲み（13.4）の padding の 24 に収まり、字に掛からない。
輪の色は `--paper` で、地（ink、ink-2）に対し 3 : 1 以上である（13.5 の表）。輪の太さ 2 は、footer の輪（6.5）と同じである。
つまみ、hover の線、drag の線は、境目が動かせると見分ける手がかりで、これも 3 : 1 以上である。通常の線（`--line`）は装飾の罫線で、手がかりはつまみが担う。

### 13.3 入れ替えの button と選ぶ口

操作の並びには `role="toolbar"` を付けない。
付けると矢印の key で部品の間を動く約束になり、設計の「Tab で focus を移す」と食い違うからである。

| 部品 | 寸法 | 見た目 |
|---|---|---|
| 操作の並び | 高さ 45。`display: flex; flex-wrap: wrap; align-items: center; gap: 8px 16px; padding: 8px 24px`。下に 1 px の `--line` | 左から、入れ替えの button、側の組 2 つ（左の列の側が先）、空き（`flex: 1`）、switch（13.5） |
| button | 高さ 28、`min-width` 28、padding 0 × 10、r-3（6）、1 px の枠 | 字は sans の fs-label（12）、`--paper`。地は無し。枠は `--paper-faint` |
| `select` | 高さ 28、`min-width` 136、padding 0 × 8、r-3、1 px の枠 | 字は sans の fs-label、地は `--ink`、枠は `--paper-faint`。矢印は native（画像を使わない） |
| 倍率の値 | `min-width` 5ch、中央寄せ | mono の fs-label、`--paper`。`aria-hidden="true"`。読み上げは live の文（下の表の `scale_live`） |
| 側の組 | `display: inline-flex; align-items: center; gap: 8px`、`role="group"` | 先頭に言語の印（4.1）、`select`、下げる button（「−」）、倍率の値、上げる button（「+」）、既定の button。2 つ目の組は、左に padding 16 と 1 px の `--line` の縦線 |
| 開閉の欄（860 px 以下） | summary は button と同じ寸法 | 字の後ろに chevron（6 × 6 の枠線 2 px を 45 度回す。開くと向きを返す）。開いた中は地 `--ink-2`、1 px の `--line`、r-3、padding 8 × 12、上の余白 8。中は側の組を縦に積み、`select` は幅いっぱい、倍率の 3 つは次の行 |

| 状態 | 枠 | 地 | 字 |
|---|---|---|---|
| 通常 | 1 px、`--paper-faint` | なし | `--paper` |
| hover | `--paper` | なし | `--paper` |
| `:focus-visible` | 通常のまま、外に `outline: 2px solid var(--paper); outline-offset: 2px` | | |
| `aria-pressed="true"`（入れ替え済み） | `--paper` | `--paper` | `--ink` |
| `disabled`（80% の下げる、200% の上げる、決められない時の決める） | 1 px の破線、`--line` | なし | `--paper-faint` |

入れ替えの button の字は、押しても変えない。押した状態は地を塗って示し、色だけに頼らない。
`disabled` の字は `--paper-faint`（暗い 5.14、明るい 5.31）で読めるまま、破線の枠で押せないと分かる。
字体と大きさの口は `/dashboard` だけに置く。`/transcript` の `refine-text` は 18 px（`css.rs:188`）で、設計の 4 つの文字の大きさの表（16 px を基準にした 12.8 から 48 px）と値が違うからである。

| 部品 | key | ja | en |
|---|---|---|---|
| 入れ替えの button（字と `aria-label`） | `ui.layout.swap` | 左右を入れ替える | Swap sides |
| 縦の境目（`aria-label`） | `ui.layout.split_cols` | 左右の列の幅 | Width of the left and right columns |
| 横の境目（`aria-label`） | `ui.layout.split_rows` | 確定フィードと清書の高さ | Heights of the final feed and the refined text |
| 側の組（`aria-label`） | `ui.text.group` | {lang} の字幕の文字 | Subtitle text for {lang} |
| 字体の `select`（`aria-label`） | `ui.text.font` | {lang} の字体 | Font for {lang} |
| 字体の選択肢 `default` | `ui.text.font_default` | 標準 | Default |
| 下げる button（`aria-label`） | `ui.text.smaller` | {lang} の字を小さくする | Make {lang} text smaller |
| 上げる button（`aria-label`） | `ui.text.larger` | {lang} の字を大きくする | Make {lang} text larger |
| 既定の button（字） | `ui.text.reset` | 既定 | Default |
| 既定の button（`aria-label`） | `ui.text.reset_label` | {lang} の字の大きさを既定に戻す | Reset {lang} text size to default |
| 倍率の live の文 | `ui.text.scale_live` | {lang} の字の大きさ {pct}% | {lang} text size {pct}% |
| 開閉の欄の summary | `ui.text.open` | 文字の設定 | Text settings |
| switch（字と `aria-label`） | `ui.theme.switch` | 暗い表示 | Dark display |
| Listening の印 | `ui.dashboard.quiet` | 静か | Quiet |

`{lang}` と `{pct}` の置き換えは、`{n}`（`ui.common.dropped`）と同じである。
`{lang}` には言語の code をそのまま入れる（wire の値で、切り替えない。設計 3.14）。
字体の名前（`cinecaption`、`UDEV Gothic 35`、`Yu Gothic UI`、`Meiryo`、`Yu Mincho`）は固有名で、文言にしない。
既定の button の字と `aria-label` は、字の「既定」が名前に含まれる形にそろえた（WCAG 2.2 の 2.5.3）。

### 13.4 行の寄せ方

| 項目 | 決め |
|---|---|
| 寄せ | 左の列も右の列も左寄せ（`text-align: start`）。右の列を右寄せにして鏡に写す形は採らない |
| 理由 | 右寄せは行の左端がそろわず、2 行に折れた原文と訳の行頭を探すことになる。どちらの側かは、列の位置と言語の印が示すので、寄せで重ねて示さない |
| 行の内側 | 6.2 のまま（`auto 1fr auto`、gap 12）。言語の印と話者の印は左端、原文はその右、遅れは右端（4.1） |
| 行の囲み | 各行は、列の幅いっぱいで、左右に padding 24 を持つ囲みに入れる。囲みを container にし、囲みの内側の幅が 340 px 以下では、行を 3 段にする（1 段目は印と遅れ、2 段目は原文、3 段目は訳）。最小の列の幅 240 でも、1 行が 4 字になる形を避ける |
| 区切り | 各行の下の破線（6.2 の `--line-soft`）は、行が入った列の幅にだけ引く。反対の側は空ける。破線が段を踏む形に連なり、話の入れ替わりが見える（設計 10.13.10 の H59 の空きは、この形で手がかりにする） |
| 組を変えた所 | 14.2 の区切りの行が、2 つの列と縦の線にまたがる |
| 1 列に戻った時（860 px 以下） | 左右の囲みを 1 列にそろえる。左端と padding はそのまま。時刻の順に並べ、言語の印で読み分ける（今の形）。縦の線と入れ替えは出さない |

### 13.5 明るい表示と暗い表示

設計 10.13.11 が決めた値（`--ink`、`--ink-2`、`--paper`、`--paper-dim`、`--paper-faint`、`--paper-quiet`）は変えない。
この板が決めたのは、`--line`、`--line-soft`、`--shu`、言語の色 6 つの、明るい表示の値 9 件である。
言語の色と `--shu` は、暗い表示と同じ色相のまま、`--ink-2` の上で字として 4.5 : 1 以上になるまで暗くした。
`--c-ja` は `--shu` と同じ値にする（暗い表示と同じ。ja は朱）。
`--line` は、地（ink）に対する比を暗い表示の 1.43 にそろえ、罫線の重みを替えない。

| 名前 | 暗い表示（3.1） | 明るい表示 | 出所 |
|---|---|---|---|
| ink | `#0d0f13` | `#f7f4ee` | 設計 10.13.11 |
| ink-2 | `#14171d` | `#ece7dd` | 設計 10.13.11 |
| line | `#2a2f3a` | `#d4cec1` | この板 |
| line-soft | `rgba(42,47,58,.7)` | `rgba(212,206,193,.7)` | この板 |
| paper | `#ece5d8` | `#1a1c21` | 設計 10.13.11 |
| paper-dim | `#b0a898` | `#4a4d55` | 設計 10.13.11 |
| paper-faint | `#8a8473` | `#62656d` | 設計 10.13.11 |
| paper-quiet（新しい token） | `#8a8473` | `#62656d` | 設計 10.13.13 の 13 |
| shu | `#e04f2f` | `#b8381c` | この板 |
| c-ja | `#e04f2f` | `#b8381c` | この板 |
| c-en | `#5b7fd4` | `#3d60b3` | この板 |
| c-zh | `#d4a13c` | `#835e10` | この板 |
| c-ko | `#3fae9d` | `#167163` | この板 |
| c-yue | `#b06bc6` | `#8a46a2` | この板 |
| c-xx | `#7a8194` | `#5e6577` | この板 |

`TOKENS` は、次の 3 つの規則に分ける。
明るい表示の色の組は 1 つの定数にして、2 か所へ差す。
`data-theme` が無く、端末も明るい表示でなければ、暗い表示（今と同じ）で描く。
`/send` は `data-theme` を書かず、この `@media` だけで端末に従う。
overlay、lanes、`BASE` には `data-theme` も `color-scheme` も付けない（5 の P2 と BD-4 の透明を守る）。

```
:root, :root[data-theme="dark"] { color-scheme: dark; /* 暗い表示の色 */ }
:root[data-theme="light"] { color-scheme: light; /* 明るい表示の色 */ }
@media (prefers-color-scheme: light) { :root:not([data-theme]) { color-scheme: light; /* 明るい表示の色 */ } }
```

`color-scheme` を付けるのは、native の `select`（開いた一覧）と scrollbar を、頁の明暗に合わせるためである。
字の色の対比は、3.1 と同じ式で計算した。
次の 1 つの command が、表の 40 件の比（paper-quiet の行は paper-faint と同じ値なので除く）と、装飾の罫線の 2 件を出す。

| 前景 | 背景 | 暗い表示 | 明るい表示 | 使う所 |
|---|---|---|---|---|
| paper | ink | 15.31 | 15.53 | 本文、focus の輪、drag の線、switch の入 |
| paper | ink-2 | 14.33 | 13.83 | 帯の中の字、meter の塗り |
| paper-dim | ink | 8.13 | 7.70 | 補助の字、hover のつまみ |
| paper-dim | ink-2 | 7.61 | 6.86 | 帯の中の補助の字、「静か」の印 |
| paper-faint | ink | 5.14 | 5.31 | 操作の枠、つまみ、switch の切、`disabled` の字 |
| paper-faint | ink-2 | 4.81 | 4.73 | 速報の訳の薄い部分（帯の中）、meter の枠 |
| paper-quiet | ink-2 | 4.81 | 4.73 | 静まった欄の字（`paper-faint` と同じ値、同じ比） |
| shu | ink | 4.87 | 5.28 | `LIVE`、`REFINED`、caret の字と塗り |
| shu | ink-2 | 4.55 | 4.71 | 帯の中の朱 |
| c-ja | ink-2 | 4.55 | 4.71 | 訳の行の矢印と言語の字 |
| c-en | ink-2 | 4.64 | 4.84 | 同上 |
| c-zh | ink-2 | 7.66 | 4.77 | 同上 |
| c-ko | ink-2 | 6.63 | 4.76 | 同上 |
| c-yue | ink-2 | 4.93 | 4.90 | 同上 |
| c-xx | ink-2 | 4.61 | 4.73 | 同上 |
| ink | c-ja | 4.87 | 5.28 | 言語の印の字 |
| ink | c-en | 4.96 | 5.43 | 同上 |
| ink | c-zh | 8.19 | 5.35 | 同上 |
| ink | c-ko | 7.08 | 5.35 | 同上 |
| ink | c-yue | 5.27 | 5.50 | 同上 |
| ink | c-xx | 4.93 | 5.31 | 同上 |

すべて 4.5 以上である（字の下限）。
部品の見た目（輪、つまみ、枠、switch）の下限は 3 で、同じ表の paper と paper-faint の行が満たす。
`line` と `ink` の比は暗い表示も明るい表示も 1.43 で、装飾の罫線なので下限を置かない。

```sh
awk 'function hx(c){return index("0123456789abcdef",tolower(c))-1}
function ch(h,i,  v){v=(hx(substr(h,i,1))*16+hx(substr(h,i+1,1)))/255; return v<=0.03928?v/12.92:((v+0.055)/1.055)^2.4}
function L(h){return 0.2126*ch(h,2)+0.7152*ch(h,4)+0.0722*ch(h,6)}
{a=L($2); b=L($3); if(a<b){t=a;a=b;b=t} r=(a+0.05)/(b+0.05); printf "%-26s %s %s %6.2f %s\n",$1,$2,$3,r,(r>=$4?"ok":"NG")}' <<'EOF'
dark:paper/ink          #ece5d8 #0d0f13 4.5
light:paper/ink         #1a1c21 #f7f4ee 4.5
dark:paper/ink-2        #ece5d8 #14171d 4.5
light:paper/ink-2       #1a1c21 #ece7dd 4.5
dark:paper-dim/ink      #b0a898 #0d0f13 4.5
light:paper-dim/ink     #4a4d55 #f7f4ee 4.5
dark:paper-dim/ink-2    #b0a898 #14171d 4.5
light:paper-dim/ink-2   #4a4d55 #ece7dd 4.5
dark:paper-faint/ink    #8a8473 #0d0f13 4.5
light:paper-faint/ink   #62656d #f7f4ee 4.5
dark:paper-faint/ink-2  #8a8473 #14171d 4.5
light:paper-faint/ink-2 #62656d #ece7dd 4.5
dark:shu/ink            #e04f2f #0d0f13 4.5
light:shu/ink           #b8381c #f7f4ee 4.5
dark:shu/ink-2          #e04f2f #14171d 4.5
light:shu/ink-2         #b8381c #ece7dd 4.5
dark:c-ja/ink-2         #e04f2f #14171d 4.5
light:c-ja/ink-2        #b8381c #ece7dd 4.5
dark:c-en/ink-2         #5b7fd4 #14171d 4.5
light:c-en/ink-2        #3d60b3 #ece7dd 4.5
dark:c-zh/ink-2         #d4a13c #14171d 4.5
light:c-zh/ink-2        #835e10 #ece7dd 4.5
dark:c-ko/ink-2         #3fae9d #14171d 4.5
light:c-ko/ink-2        #167163 #ece7dd 4.5
dark:c-yue/ink-2        #b06bc6 #14171d 4.5
light:c-yue/ink-2       #8a46a2 #ece7dd 4.5
dark:c-xx/ink-2         #7a8194 #14171d 4.5
light:c-xx/ink-2        #5e6577 #ece7dd 4.5
dark:ink/c-ja           #0d0f13 #e04f2f 4.5
light:ink/c-ja          #f7f4ee #b8381c 4.5
dark:ink/c-en           #0d0f13 #5b7fd4 4.5
light:ink/c-en          #f7f4ee #3d60b3 4.5
dark:ink/c-zh           #0d0f13 #d4a13c 4.5
light:ink/c-zh          #f7f4ee #835e10 4.5
dark:ink/c-ko           #0d0f13 #3fae9d 4.5
light:ink/c-ko          #f7f4ee #167163 4.5
dark:ink/c-yue          #0d0f13 #b06bc6 4.5
light:ink/c-yue         #f7f4ee #8a46a2 4.5
dark:ink/c-xx           #0d0f13 #7a8194 4.5
light:ink/c-xx          #f7f4ee #5e6577 4.5
dark:line/ink(decor)    #2a2f3a #0d0f13 0
light:line/ink(decor)   #d4cec1 #f7f4ee 0
EOF
```

出力に `NG` の行は無い（`... | rg -c ' NG$'` は一致 0 件で、rc は 1）。
陰性対照は、`echo 'x #62656d #4a4d55 4.5' | awk '<上の式>'` が `1.45 NG` を返すことである。

規則の側に残る色の直書きは次の 2 つで、token から作る形に直す。

| 場所 | 今の値 | 直す形 |
|---|---|---|
| `css.rs:28` の `pulse` | `rgba(224,79,47,.5)` と `rgba(224,79,47,0)` | `color-mix(in srgb, var(--shu) 50%, transparent)` と `color-mix(in srgb, var(--shu) 0%, transparent)`。明るい表示の朱でも輪が合う |
| `css.rs:155` の `.strip.quiet` | `opacity: .5` と `--paper-dim`（合成した比が暗い 2.86、明るい 2.37） | 設計 10.13.13 の 13 のとおり、色を `--paper-quiet` に、`opacity` を 1 に、`.strip-text` の `font-weight` を 400 にする。値はこの板が足さない |

Listening の欄の「静か」の印は、欄の見出し（`strip-label`）の後ろに gap 12 で置く。
形は 4.2 の「先出し」の印（`.tr-mark`）と同じで、字間は 0（見出しの .35em を継がない）、字は `--paper-dim`（暗い 7.61、明るい 6.86、`--ink-2` の上）である。
出すのは `strip_quiet` の間だけで、待ちの文言（`ui.dashboard.waiting`）を出している間は出さない（文言が状態を言っている）。

| 部品 | 値 |
|---|---|
| switch の要素 | 1 つの `button`（`role="switch"`）。高さ 28、padding 0 × 4 × 0 × 8、gap 8。中に字と track |
| 字 | `ui.theme.switch`（13.3）、fs-label、`--paper`。状態で変えない（APG の switch） |
| track | 36 × 20、1 px の枠 `--paper-faint`、r-2（4） |
| knob | 14 × 14、r-1（2）。track の中で上下左右に 2 空ける |
| 切（`aria-checked="false"`。明るい表示） | track は地なし、knob は左で `--paper-faint` |
| 入（`aria-checked="true"`。暗い表示） | track は地も枠も `--paper`、knob は右へ 16 動いて `--ink` |
| `:focus-visible` | `outline: 2px solid var(--paper); outline-offset: 2px` |
| 置き場 | 操作の並びの右端（`margin-left: auto`）。860 px 以下でも隠さない。`/transcript` では、頁の先頭に右寄せで switch だけを置く（高さ 28、下の余白 16） |

| 動き | 値 | 使う所 | 止める条件 |
|---|---|---|---|
| knob | 120 ms ease-out の `transform` | switch の knob（16 px） | `prefers-reduced-motion: reduce`（3.3 の一括の規則が止める。動かさず替える） |
| 色の切り替え | 動かさない | 暗い表示と明るい表示の切り替え | なし |

色の切り替えを動かさないのは、途中の色で字と地の比が 4.5 を割りうるからである。
切り替えは準備の時に 1 回なので、一度に替える。
`tone`（3.3、160 ms）は先の訳が確定の訳に替わる所のためで、切り替えのために消さない。

### 13.6 受入と残る穴

| ID | 何を測るか | 期待値 | 陰性対照 |
|---|---|---|---|
| BD-13 | 13.5 の表の全組の対比 | 明るい表示の全組が下限（字は 4.5）以上で、上の command の再計算が表と ± 0.01 で一致する。`tests/board.rs` の `colours()` は、組ごとに `TOKENS` を読む（今は全部を 1 つの表へ入れ、後の組が前の組を上書きする） | `--c-xx` を暗い表示の値 `#7a8194` にした明るい表示で、ink-2 の上の比が 4.5 を割って落ちる |
| BD-14 | 操作の部品と境目の的の大きさ | 1440 × 900 で、操作の並びの button、`select`、switch の高さが 28 以上、幅が 28 以上。縦の線の中心から左右 12 px 内の点の `elementFromPoint` が境目の要素で、14 px 外の点は別の要素。操作の並びの高さ 45 ± 1 | 高さ 24 未満にした build で落ちる |
| BD-15 | focus の輪 | key で focus を置いた操作の部品と境目の computed が、`outlineWidth` 2px、`outlineStyle` solid、`outlineColor` が `--paper` の値（暗い `rgb(236, 229, 216)`、明るい `rgb(26, 28, 33)`） | `outline: none` にした build で落ちる |
| BD-16 | 左右の寄せと 3 段への切り替え | 両方の列の `row-text` の computed の `textAlign` が `start` か `left`。囲みの内側の幅が 340 px 以下の行で、`row-text` の上端が言語の印の下端以下 | 右の列を `right` にした build で落ちる |
| BD-17 | 動きの停止 | `prefers-reduced-motion: reduce` で、switch の knob の `transitionDuration` が `0s`、`pair-divider`（14.2）の `animationName` が `none` | 指定を外した build で落ちる |

| 番号 | 穴 | 持ち主と置き直す条件 |
|---|---|---|
| W13-1 | 操作の並びの幅（約 1030 px で 1 行）と段の高さは、板の値で組んだ mock の実測で、実装した頁の値と、読みやすさ（設計 10.13.10 の H59）は NOT VERIFIED | IP-S12 の席が BD-14 で測り、違えば 13.1 の表を直す |
| W13-2 | 明るい表示の色は、対比だけを計算した。色覚の違う人の見え方と、明るい部屋での見え方は測っていない（言語の印は字でも読めるので、色だけに頼らない）（NOT VERIFIED） | 人が開いた頁で確かめた席が、違えば 13.5 の表を直す |

## 14. 言語の組の頁（IP-S15、設計 10.13.12）

IP-S15 が写す見た目を決める。
選ぶ口の要素（`.pair-picker`、`#pair-a`、`#pair-b`、`#pair-apply`）と区切りの行（`.pair-divider`、`data-pair`）の名前は、設計 10.13.13 の 12 が決めている。
利用者は、会話の 2 つの言語を決める人（loopback の頁）と、その字幕を読む人（LAN の頁）である。
1 つの仕事は、今の組を取り違えずに読めることと、組を変えた所が行の中で分かることである。

| 案 | 構造 | 判定 |
|---|---|---|
| A | 2 つの `select`（1 つめの言語、2 つめの言語）と、決める button。言語を選んでから決めるまで、組は変わらない | 採る（設計 10.13.13 の 12 の要素の名前がこの形） |
| B | 言語の印を並べた button の対。印を押すと組の 1 つが替わり、すぐ効く | 採らない |

B を採らない理由は、組を替えると認識器を読み直し（設計 10.13.12 の 4）、会話の字幕が止まるからである。
押しただけで効く形では、2 つの言語を選ぶ途中の組でも読み直しが走る。2 つを選んでから 1 回で決める形に分ける。

### 14.1 選ぶ口と示すだけの形

| 項目 | 値 |
|---|---|
| 置き場 | header の右側、chip の前。会話の性質（設計 10.13.12）で、頁を見る人ごとの設定（13.3 の操作の並び）と分ける |
| 構成 | 小見出し「会話の言語」（fs-label、`--paper-dim`）、`select#pair-a`、`select#pair-b`、`button#pair-apply`。gap 8、`align-items: center`。枠は付けない |
| `select` | 13.3 の `select` の寸法（高さ 28）。字は mono の fs-label、`min-width` 0、padding 0 × 6。選択肢の字は言語の code（`en`、`ja`）そのもの |
| 決める | 13.3 の button |
| 選択肢 | a は、選べる組に入る言語。b は、a と組める言語（a 自身を除く）。a を替えて b が組めなくなったら、b は組める最初の言語に替える |
| 通常 | 2 つの `select` が今の組を指し、決めるは `disabled` |
| 替えた | 今の組と違う組を指すと、決めるが有効になる |
| 反映中 | 決めるの字が「切り替え中…」（`ui.pair.applying`）になり、3 つとも `disabled`。認識器を読み終えて `langs` が届くと、通常に戻る |
| 失敗 | 欄の右に 1 つの印「切り替えられませんでした」を `role="alert"` で出す（1 px の破線の枠 `--paper`、r-1、fs-label）。次に `select` を触るまで出す。時間で消さない |
| 示すだけの形 | 相手の address が loopback でない頁（LAN の頁）、選べる組が 1 つの頁、`/langs` を読めない頁。小見出し「会話の言語」と、組の 2 つの言語の印（4.1）を gap 4 で並べる。`select` も button も出さず、`.pair-picker` は 0 個。並びは組の並び（`<a>`、`<b>`）で、入れ替え後の左右ではない |

### 14.2 組を変えた所の区切りの行

| 項目 | 値 |
|---|---|
| 要素 | `div.pair-divider`、`data-pair="<a>,<b>"`、`role` は付けない。確定フィードの格子の 1 行で、`grid-column: 1 / -1`（2 つの列と縦の線にまたがる） |
| 中身 | 中央に「会話の言語を変えました」（`ui.pair.changed`）と、新しい組の言語の印 2 つ（4.1。code が字）。左右に 1 px の破線（`--line`）が伸びる |
| 字 | fs-label、`--paper-dim`（暗い 8.13、明るい 7.70、ink の上） |
| 中央の囲み | 地は `--ink`、padding 0 × 12、縦の線より前面にして、縦の線が字を横切らない。`pointer-events: none` で、囲みの下の縦の線のつかむ領域を残す |
| 寸法 | padding 8 × 24。高さ 約 33 |
| 出る時 | `rise`（3.3、120 ms）。`prefers-reduced-motion` で止める。`css.rs` の test は `animation: rise 120ms ease-out backwards` を 2 つと数えるので、3 つに直す |
| 読み上げ | `role="log"` の中なので、新しい行として polite に読まれる |
| 入れない時 | 組の順だけを変えた時（設計 10.13.13 の 7） |

### 14.3 組の言語の色

| 言語 | 印の地 | 備考 |
|---|---|---|
| ja、en、zh、ko、yue | 3.1 と 13.5 の色 | |
| それ以外（es、fr、de ほか） | `--c-xx` | 字は code そのもの（`es`）。`??` は `lang` が空の行だけが持つ |
| 組の 2 つとも `--c-xx`（`es,fr`） | 両方 `--c-xx` | 色で見分けず、字で見分ける。色は足さない（6 つの色の表の外を増やすと、地と字の比を測り直す表が増える） |

| 部品 | key | ja | en |
|---|---|---|---|
| 欄の小見出し | `ui.pair.label` | 会話の言語 | Conversation languages |
| `select#pair-a`（`aria-label`） | `ui.pair.first` | 1 つめの言語 | First language |
| `select#pair-b`（`aria-label`） | `ui.pair.second` | 2 つめの言語 | Second language |
| `button#pair-apply` | `ui.pair.apply` | 決める | Apply |
| 反映中の字 | `ui.pair.applying` | 切り替え中… | Switching… |
| 失敗の印 | `ui.pair.failed` | 切り替えられませんでした | Could not switch |
| 区切りの行 | `ui.pair.changed` | 会話の言語を変えました | Conversation languages changed |

| ID | 何を測るか | 期待値 | 陰性対照 |
|---|---|---|---|
| BD-18 | 区切りの行の位置 | `.pair-divider` の左端が格子の左端以下、右端が格子の右端以上（2 つの列にまたがる）、`role` 属性が無い | 1 つの列だけにまたがる build で落ちる |
| BD-19 | 示すだけの形 | LAN の address で開いた頁の `.pair-picker` が 0 個で、組の言語の印が `<a>`、`<b>` の順に 2 つ | loopback で開いた頁は `.pair-picker` が 1 個（陽性対照） |
| BD-20 | 組の言語の色 | `es` を含む組の印が `c-xx` の地で、字が `es` | `lang` が空の行の印は `??`（今の形） |

## 15. 音を送る頁（IP-S13、設計 10.13.6 の `/send`）

IP-S13 が写す見た目を決める。
`/send` の中身（音源は 2 つ、利用者が押した時に取る、音の track が無ければ送らない、切れたら繋ぎ直さない）は設計 10.13.6 が決めている。
利用者は、別の PC で `/send` を開く人である。hayamimi を動かす人本人とは限らない。
1 つの仕事は、自分の PC の音を迷わず送り始め、送れていることを確かめ、止められることである。
語りの調子は、状態が一番先に見えることである。
`/send` は 3 つの頁と同じ token（13.5 の 2 つの組）を使う。
`data-theme` は書かず、`prefers-color-scheme` に従う（switch は置かない）。`localStorage` は使わない。
sheet は `BASE`、`TOKENS`、`MOTION` と、この節の `SEND` である。`PARTS` は使わない。

**最新情報**：MDN は、`getDisplayMedia()` が `audio: true` でも音の track の無い stream を返しうること、利用者の操作（transient activation）を要し、無い時は `InvalidStateError` を投げること、拒否は `NotAllowedError`、source が無い時は `NotFoundError`、OS の側の失敗は `NotReadableError` と書く（https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getDisplayMedia 、取得 2026-10-29）。
同じく `getUserMedia()` は、insecure な context で `navigator.mediaDevices` が `undefined` になり、`TypeError` になりうる（https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia 、取得 2026-10-29）。
Chrome の画面共有の頁（2022-09-29 更新）は、system audio を共有する check box を入れても、相手に音が届かず利用者が混乱する場面があると書く（https://developer.chrome.com/docs/web-platform/screen-sharing-controls 、取得 2026-10-29）。
MDN の `role="meter"` は、native の `<meter>` を勧め、`aria-valuenow` を変える時は見た目の幅も更新することを求める（https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Reference/Roles/meter_role 、取得 2026-10-29）。

| 案 | 構造 | 判定 |
|---|---|---|
| A | 1 列。状態の文を一番上に置き、次に音の元の button、入力の大きさ、合言葉と送り先、注意 | 採る |
| B | 2 つの音の元を、画面を左右に分けた 2 枚の面にし、それぞれに button と meter と状態を持たせる | 採らない |

B を採らない理由は、音を同時に送れるのは 1 本だけ（設計 10.13.13 の 8）で、2 つの面が同時に動くように見えるからである。
送っていない面の meter は空のまま残り、どちらが今効いているかを読み取る手間が面の数だけ増える。

### 15.1 配置

```
┌──────────────────────────────────────────────────────────────────┐
│ hayamimi                                                         │  romaji（fs-label、字間 .3em）
│ 音を送る                                                         │  h1（serif、fs-strip）
│                                                                  │
│ ● 送っています                                                   │  状態の文（serif、fs-strip）
│   マイクの音を送っています。この頁を開いたままにしてください。   │  補足（fs-sub）
│                                                                  │
│ [マイク]  [tab か画面の音]  [止める]                             │  button（高さ 44）
│ tab や画面の音は、共有の画面で、音声も共有する項目に…            │  助言（fs-label）
│                                                                  │
│ 入力の大きさ                                                     │  小見出し
│ [███████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░]                     │  meter（高さ 12）
│                                                                  │
│ 送り先  http://192.168.1.5:8833                                  │  情報（dl、fs-label）
│ 合言葉  受け取りました                                           │
│ 使い終えたら、flag に登録した origin を消してください。          │  注記（fs-label）
└──────────────────────────────────────────────────────────────────┘
```

| 領域 | 寸法と見た目 |
|---|---|
| 根 | 地は ink、字は paper、padding 32 × 24（幅 520 px 以下は 24 × 16）、最小の高さ 100vh、font-sans、fs-body。`max-width: 640px; margin-inline: 0`（左寄せ。7 の transcript と同じ流儀） |
| 頭 | romaji `hayamimi`（fs-label、字間 .3em、`--paper-dim`）の下に h1「音を送る」（serif、fs-strip の 24 / 600 / 1.5、下の余白 24） |
| 状態の文 | 15.2 |
| 操作 | 状態の文の下、上の余白 24。button は gap 12 で横に並べ、折り返す。下に助言（fs-label、`--paper-dim`、上の余白 12） |
| 入力の大きさ | 15.5。上の余白 32 |
| 合言葉と送り先 | 15.4。上の余白 32 |
| 注意と注記 | 15.6 |

要素の名前は、設計 10.13.13 の 12 と同じ流儀で、test-designer が使う名前をここで決める。
`#send-status`、`#send-mic`、`#send-display`、`#send-stop`、`#send-token`、`#send-meter`、`#send-origin`（注意の欄の origin の入力欄）である。
音の元の button の字は、設計の受入が押す「マイク」と同じ字にする。

### 15.2 状態と音の元の button

状態は 6 つで、状態の文を 1 つだけ出す。

| 状態 | 印 | 状態の文と補足の key | 音の元の 2 つ | 止める |
|---|---|---|---|---|
| つながっていない | 点 `--c-xx` | `ui.send.state.idle`、`ui.send.lead.idle` | 有効 | `disabled` |
| 選んでいる（許可の画面が出ている間） | 点 `--c-xx` | `ui.send.state.choosing`、`ui.send.lead.choosing` | `disabled` | `disabled` |
| つなげている | 点 `--shu`、pulse | `ui.send.state.connecting`（補足なし） | `disabled` | 有効 |
| 送っている | 点 `--shu`、pulse | `ui.send.state.sending`、`ui.send.lead.sending_mic` か `ui.send.lead.sending_display` | `disabled` | 有効 |
| 止まった | 点 `--c-xx` | `ui.send.state.stopped`。利用者が止めた時は補足なし、切れた時は `ui.send.lead.lost` | 有効 | `disabled` |
| 誤り | 「!」の印 | `ui.send.state.failed`、補足は 15.3 の文 | 15.3 のとおり | `disabled` |

| 部品 | 寸法 | 見た目 |
|---|---|---|
| 点 | 10 × 10 の円（6.1 の接続の点と同じ） | `--c-xx` は動かない。`--shu` は pulse（3.3）で、`prefers-reduced-motion` で止める |
| 「!」の印 | 20 × 20、1 px の破線 `--paper`、r-1 | mono の fs-label、weight 600 の「!」。色を使わない（言語の色の朱との取り違えを避ける。4.3 と同じ理由） |
| 状態の文 | serif、fs-strip（24 / 600 / 1.5） | `--paper`。点か印の右、gap 12 |
| 補足 | fs-sub（15 / 1.5）、左の余白 22、上の余白 4 | 誤りの時は `--paper`、ほかは `--paper-dim` |
| 音の元の button、止める button | 高さ 44、padding 0 × 20、r-3、1 px の枠、字は sans の fs-sub、weight 600 | 有効は、地 `--paper`、字 `--ink`、枠 `--paper`。`disabled` は、地なし、1 px の破線 `--line`、字 `--paper-faint` |
| focus | | `outline: 2px solid var(--paper); outline-offset: 2px` |

状態の文の領域は、誤り以外は `role="status"`、誤りは `role="alert"` にする。
送っている間に補足が「この頁を開いたままにしてください」と言うのは、頁を閉じると送りが止まるからである。
`getDisplayMedia` を持たない browser では、「tab か画面の音」を `disabled` にし、下に `ui.send.display_unsupported`（fs-label、`--paper-dim`）を出す。マイクは使える。

### 15.3 誤りの文

クライアントから見える形は、設計 10.13.6 の合言葉の断り（close code 1008）と、10.13.13 の 8 の 2 本目の断り（close code 1013）に合わせる。

| 原因 | 見える形 | key | ja | en |
|---|---|---|---|---|
| `denied` | `NotAllowedError` | `ui.send.err.denied` | ブラウザの許可が出ませんでした。許可して、もう一度押してください。 | The browser did not allow it. Allow it, then press again. |
| `not_found` | `NotFoundError` | `ui.send.err.not_found` | 送れる音の元が見つかりません。 | No source of sound was found. |
| `not_readable` | `NotReadableError`、`AbortError` | `ui.send.err.not_readable` | 音の元を使えませんでした。ほかの app が使っていないか、確かめてください。 | The source could not be used. Check that no other app is using it. |
| `no_audio` | `getDisplayMedia` の stream に音の track が無い | `ui.send.err.no_audio` | 音が選ばれていません。共有の画面で、音声も共有する項目にチェックを入れて、もう一度押してください。 | No audio was selected. In the share dialog, tick the item that also shares audio, then press again. |
| `token` | `ready` の前に close code 1008 | `ui.send.err.token` | 合言葉が違います。送り先の PC の /dashboard にある案内から、開き直してください。 | The passphrase is wrong. Open this page again from the guide on the /dashboard of the PC running hayamimi. |
| `busy` | close code 1013 | `ui.send.err.busy` | ほかの端末が音を送っています。 | Another device is already sending. |
| `refused` | upgrade が 101 でない、`ready` の前に 1008 と 1013 以外で閉じた、開けなかった | `ui.send.err.refused` | つながりません。同じ LAN にいるか、頁を /dashboard にある URL から開いているかを、確かめてください。 | Could not connect. Check that you are on the same LAN and opened this page from the URL on the /dashboard. |
| `other` | 上のどれでもない例外（`InvalidStateError` を含む） | `ui.send.err.other` | 送れませんでした。もう一度押してください。 | Could not send. Press again. |

`token` の後は 15.4 の入力欄を出し、合言葉が 32 字の小文字の 16 進になるまで、音の元の 2 つを `disabled` にする。
ほかの誤りは、音の元の 2 つを有効にして、押し直しで試せる。
`ready` の後に切れた時は、誤りでなく「止まった」の `ui.send.lead.lost` で、利用者がもう一度押すまで繋ぎ直さない。

### 15.4 合言葉と送り先

| 項目 | 値 |
|---|---|
| 受け取る | URL の fragment（設計 10.13.6）。32 字の小文字の 16 進なら受け取り済みとし、入力欄を出さない。読んだら `history.replaceState` で fragment を消す |
| 受け取り済み | 「送り先」の情報の「合言葉」の行に「受け取りました」（`ui.send.token_ok`）。合言葉の字は画面に出さない |
| 入力欄を出す時 | fragment が無い、形が違う、頁を読み直して fragment が消えた、`token` の誤りの後 |
| 入力欄 | `#send-token`、`type="password"`、`autocomplete="off"`、`autocapitalize="off"`、`autocorrect="off"`、`spellcheck="false"`、`maxlength` 64。高さ 40、padding 0 × 12、r-3、1 px の枠 `--paper-faint`、地 `--ink`、幅 100%。字は mono の 16（16 より小さいとスマートフォンの browser が拡大する） |
| 見出しと数 | 上に小見出し「合言葉」（fs-label、字間 .35em、`--paper-dim`）、右端に「{n} / 32」（mono の fs-label） |
| 助言 | 下に fs-label、`--paper-dim`（`ui.send.token_hint`）。上の余白 4 |
| 形の誤り | 小文字の 16 進でない字が 1 字でもあれば、枠を 1 px の破線 `--paper` にし、助言を `ui.send.token_format` に替える。大文字は黙って小文字にしない（設計 10.13.6 は大文字を違う合言葉として断る）。貼り付けた前後の空白は削る |
| start の有効 | 入力欄が出ている間は、32 字の小文字の 16 進になるまで、音の元の 2 つは `disabled` |
| 送り先 | `dl`（fs-label、2 列、gap 4 × 16）。名前は `--paper-dim`、値は mono、`--paper`。送り先は `location.origin`、合言葉は「受け取りました」か、入力欄が出ている間は行ごと出さない |

### 15.5 入力の大きさ

| 項目 | 値 |
|---|---|
| 見出し | 「入力の大きさ」（`ui.send.level`。fs-label、字間 .35em、`--paper-dim`） |
| 枠 | 幅 100%、高さ 12、1 px の枠 `--paper-faint`、r-1、地 `--ink-2`、`overflow: hidden`。上の余白 8 |
| 塗り | 高さ 100%、`--paper`、`transform: scaleX(var(--level))`、`transform-origin: left`。`--level` は 0 から 1 |
| 値 | `--level` は (dBFS + 60) / 60 を 0 から 1 に収めた値。dBFS は 100 ms ごとの RMS（送る直前の 16 kHz、mono の標本） |
| 色 | 1 色の長さだけで読ませる。緑、黄、赤に分けない（13.1 の棚卸し） |
| 止まっている間 | 塗りは 0 |
| 音が無い時の印 | 送っている間に、−60 dBFS 未満が 10 秒続いたら、枠の下に印「音が入っていません」（`ui.send.silent`。fs-label、1 px の破線の枠 `--paper`、r-1、padding 0 × 4、上の余白 8）を出す。−60 以上に戻ったら消す |
| 更新 | 100 ms ごと。`prefers-reduced-motion: reduce` では 250 ms ごと |
| 読み上げ | 枠と塗りは `aria-hidden="true"`（1 秒に 10 回の変化を読ませない）。音が無い印だけ `role="status"` |

見た目だけの部品なので、`<meter>` や `role="meter"` を使わず、`div` 2 つで組む。
値の写しと 10 秒の線は、この板が決めた値である（W15-2）。

### 15.6 secure origin の注意

`isSecureContext` が偽、または `navigator.mediaDevices` が無い時だけ、状態の文の下、音の元の button の上に出す。
この間、音の元の 2 つは `disabled` で、状態は「つながっていない」のままである。

| 項目 | 値 |
|---|---|
| 欄 | 1 px の破線 `--paper-dim`、r-2（4）、padding 12 × 16、上の余白 24 |
| 題 | `ui.send.secure.title`（fs-sub、weight 600） |
| 本文 | `ui.send.secure.lead`（fs-sub） |
| origin | `#send-origin`。読み取り専用の `input`（mono 13、高さ 36、r-3、1 px の枠 `--paper-faint`）。`user-select: all` で、1 回の click が全選択になる。copy の button は付けない（http の頁では clipboard API が使えない） |
| 手順 | 番号付き（fs-sub）。`chrome://flags/#unsafely-treat-insecure-origin-as-secure` は `code`（mono 13）で書く。link にしない（Chromium は web の頁からの `chrome://` への移動を許さない） |
| 結び | `ui.send.secure.edge` と `ui.send.secure.undo`（fs-label、`--paper-dim`） |

`isSecureContext` が真で、host が loopback でない時（flag が効いている時）は、頁の最後に `ui.send.undo_note`（fs-label、`--paper-dim`）を出す。
flag は送る側の browser の安全の設定を 1 つの origin について緩める（設計 10.13.10 の H70）ので、使い終えたら消す手順を、頁の側からも言う。

| 部品 | key | ja | en |
|---|---|---|---|
| 題 | `ui.send.secure.title` | このままでは、マイクも画面の音も取れません | Microphone and screen audio cannot be used as it is |
| 本文 | `ui.send.secure.lead` | http で開いた頁には、ブラウザが音を渡しません。このブラウザに、次の origin を 1 つだけ、安全な origin として扱わせてください。 | The browser does not give audio to a page opened over http. Make this browser treat the origin below, and only that one, as secure. |
| origin の `aria-label` | `ui.send.secure.origin` | この頁の origin | Origin of this page |
| 手順 1 | `ui.send.secure.step1` | アドレス欄に chrome://flags/#unsafely-treat-insecure-origin-as-secure と入れて開く。 | Open chrome://flags/#unsafely-treat-insecure-origin-as-secure in the address bar. |
| 手順 2 | `ui.send.secure.step2` | 上の origin を欄に入れ、Enabled にする。 | Enter the origin above in the box and set it to Enabled. |
| 手順 3 | `ui.send.secure.step3` | ブラウザを再起動して、この頁を開き直す。 | Restart the browser and open this page again. |
| 結び（Edge） | `ui.send.secure.edge` | Edge は edge://flags の同じ名前の項目です。 | In Edge, the item with the same name is in edge://flags. |
| 結び（消す） | `ui.send.secure.undo` | 使い終えたら、登録を消してください。 | Remove the entry when you have finished. |
| 頁の最後の注記 | `ui.send.undo_note` | 使い終えたら、ブラウザの flag に登録した origin を消してください。 | When you have finished, remove the origin you entered in the browser flag. |

### 15.7 狭い画面

| 項目 | 値 |
|---|---|
| 幅 521 px から | 15.1 のとおり。button は横に並べ、gap 12、高さ 44 |
| 幅 520 px 以下 | 頁の padding は 24 × 16。button は 3 つとも幅 100% で縦に並べ（マイク、tab か画面の音、止める）、高さ 48（touch の的）。状態の文は 24 のまま折り返す |
| meter、合言葉の入力欄 | 幅 100%。入力欄の高さは 44、字は 16 |
| 横 scroll | 出さない（`scrollWidth` が `clientWidth` 以下）。長い origin と URL は `overflow-wrap: anywhere` |
| `getDisplayMedia` を持たない browser | 「tab か画面の音」を `disabled` にして `ui.send.display_unsupported` を出す。マイクの button は使える |

### 15.8 dashboard の送り先の案内

設計 10.13.6 は、`/dashboard` が `/send-link` の URL と、flag に登録する origin の文字列を並べて出すと決めている。
見た目と文言は、この板が決める。

| 項目 | 値 |
|---|---|
| 出る条件 | `--send-page` で起こし、`/send-link` が 200 を返した頁（loopback の相手で `Host` が loopback の名前）だけ。ほかの頁は DOM に出さない（BR-17 の T-BR-17-018） |
| 置き場 | footer の 2 行目。footer に `flex-wrap: wrap` と `row-gap: 4px` を足す。先頭に小見出し「別の PC から音を送る」（`ui.send.guide.title`） |
| 中身 | 2 組の（名前、値、コピー）。「送る頁の URL」（`http://<LAN の address>:<port>/send#t=<合言葉>`）と「flag に登録する origin」（`http://<LAN の address>:<port>`）を、同じ行に並べる（innerText の中で 1000 字以内、T-BR-17-017） |
| 値 | mono の fs-label、`--paper`。名前は `--paper-dim`。`user-select: all`。幅が足りない時は `overflow-wrap: anywhere` |
| コピー button | 13.3 の button（高さ 28）。押すと値を clipboard へ書く。押した後の 2 秒は字を「コピーしました」にし、`aria-live="polite"` で読ませる。loopback の頁は secure context なので `navigator.clipboard` が使える |
| 合言葉 | 設計が URL の全文を出すと決めているので、伏せない |

| 部品 | key | ja | en |
|---|---|---|---|
| 小見出し | `ui.send.guide.title` | 別の PC から音を送る | Send audio from another PC |
| URL の名前 | `ui.send.guide.url` | 送る頁の URL | Page to open on the sending PC |
| origin の名前 | `ui.send.guide.origin` | flag に登録する origin | Origin to enter in the flag |
| コピー | `ui.send.guide.copy` | コピー | Copy |
| コピー後 | `ui.send.guide.copied` | コピーしました | Copied |

### 15.9 文言の表

15.2 から 15.5 と 15.7 の文言である（15.3 の誤りの文、15.6 の注意の文、15.8 の案内の文は、それぞれの節の表）。

| 部品 | key | ja | en |
|---|---|---|---|
| 題と `<title>` | `ui.send.title` | 音を送る | Send audio |
| 補足（つながっていない） | `ui.send.lead.idle` | この PC のマイクか、tab の音を、hayamimi を動かしている PC へ送ります。 | Sends this PC's microphone or tab audio to the PC running hayamimi. |
| 補足（選んでいる） | `ui.send.lead.choosing` | ブラウザの画面で、送る音を選んでください。 | Choose the audio to send in the browser's dialog. |
| 補足（送っている、マイク） | `ui.send.lead.sending_mic` | マイクの音を送っています。この頁を開いたままにしてください。 | Sending the microphone. Keep this page open. |
| 補足（送っている、画面） | `ui.send.lead.sending_display` | tab か画面の音を送っています。この頁を開いたままにしてください。 | Sending tab or screen audio. Keep this page open. |
| 補足（切れた） | `ui.send.lead.lost` | つながりが切れたので止めました。もう一度押すまで、送りません。 | The connection was lost, so sending stopped. It will not resume until you press again. |
| 状態（つながっていない） | `ui.send.state.idle` | つながっていません | Not connected |
| 状態（選んでいる） | `ui.send.state.choosing` | 選んでいます | Choosing |
| 状態（つなげている） | `ui.send.state.connecting` | つなげています… | Connecting… |
| 状態（送っている） | `ui.send.state.sending` | 送っています | Sending |
| 状態（止まった） | `ui.send.state.stopped` | 止まりました | Stopped |
| 状態（誤り） | `ui.send.state.failed` | 送れませんでした | Could not send |
| マイクの button | `ui.send.mic` | マイク | Microphone |
| 画面の音の button | `ui.send.display` | tab か画面の音 | Tab or screen audio |
| 止める button | `ui.send.stop` | 止める | Stop |
| 助言 | `ui.send.hint_display` | tab や画面の音は、共有の画面で、音声も共有する項目にチェックを入れます。 | For tab or screen audio, tick the item that also shares audio in the share dialog. |
| 画面の音が使えない | `ui.send.display_unsupported` | このブラウザは画面の音を取れません。 | This browser cannot capture tab or screen audio. |
| meter の見出し | `ui.send.level` | 入力の大きさ | Input level |
| 音が無い印 | `ui.send.silent` | 音が入っていません | No sound is coming in |
| 合言葉の見出し | `ui.send.token` | 合言葉 | Passphrase |
| 合言葉の数 | `ui.send.token_count` | {n} / 32 | {n} / 32 |
| 合言葉の助言 | `ui.send.token_hint` | 小文字の 0 から 9 と a から f の 32 字です。hayamimi を動かしている PC の /dashboard にあります。 | 32 characters, 0-9 and a-f in lower case. It is on the /dashboard of the PC running hayamimi. |
| 形の誤り | `ui.send.token_format` | 小文字の 0 から 9 と a から f だけです。 | Only 0-9 and a-f in lower case. |
| 受け取り済み | `ui.send.token_ok` | 受け取りました | Received |
| 送り先の名前 | `ui.send.dest` | 送り先 | Destination |

| ID | 何を測るか | 期待値 | 陰性対照 |
|---|---|---|---|
| BD-21 | 状態の文 | 6 つの状態のそれぞれで、`#send-status` の text が 15.2 の key の文と一致する。誤りだけ `role="alert"`、ほかは `role="status"` | 誤りを `role="status"` にした build で落ちる |
| BD-22 | 合言葉の入力 | fragment の無い URL で `#send-token` が出て、31 字の間と大文字を含む間は、音の元の 2 つが `disabled`。32 字の小文字の 16 進で有効になる。fragment の有る URL では `#send-token` が 0 個で、開いた後の `location.hash` が空 | 32 字の大文字で有効になる build で落ちる |
| BD-23 | meter | `#send-meter` が `aria-hidden="true"`。`--level` が 0 から 1。−60 dBFS 未満が 10 秒続くと「音が入っていません」が出て、`role="status"` | 9 秒で出す build で落ちる |
| BD-24 | secure でない頁の注意 | `isSecureContext` が偽の頁で、注意の欄と `#send-origin` が出て、音の元の 2 つが `disabled`。真の頁は注意の欄が 0 個 | 真の頁に注意の欄を出す build で落ちる |
| BD-25 | 狭い画面 | 390 × 844 で、button の幅が頁の幅 − 32、高さ 48 以上。`scrollWidth` が `clientWidth` 以下 | button の幅を固定した build で落ちる |
| BD-26 | 外への request | `/send` を開いて、同じ origin 以外への request が 0 件（BD-10 の続き） | 外の font を足した build で 1 件以上 |

| 番号 | 穴 | 持ち主と置き直す条件 |
|---|---|---|
| W15-1 | Edge の flag の文（`ui.send.secure.edge`）は、設計 10.13.6 の P0-4 が記録するまで NOT VERIFIED。1 つの key に分けてあるので、無い時は外すだけで済む | IP-S13 の席が P0-4 の記録を読み、違えば key と表を直す |
| W15-2 | meter の写し（−60 から 0 dBFS、100 ms の RMS）と、音が無い印の 10 秒は、この板が決めた値で、実機の音源で測っていない（NOT VERIFIED） | IP-S13 の席が BR-18 の run の記録で見て、違えば 15.5 の表を直す |

## 12. 残る穴

| 番号 | 穴 | 持ち主と置き直す条件 |
|---|---|---|
| W1 | 速報の訳は `of` が `"partial"` の `translation` で見分ける（設計 3.6 の 3）。速報の訳の `id` は、後で確定文が取る `id` である。中継が同じ入力の直前の確定文の次の番号として決めるので、入力の最初の発話では速報の訳が出ない。画面は `of` が `"partial"` の訳を、同じ `id` の確定文の行とは別の行として描く | 持ち主は R6。速報の訳を描く 1 関数を adapter に置く |
| W3 | `fallback` の `stage` と `to` の値の一覧が、設計 3.15 に「段の名前」としか無い。11 の stage の表は値が決まった分だけ足す | R2 から R8c の席が値を決めるとき、表を足す |
| W6 | font を同梱しないので、入っていない端末では代替の字体で出る。同梱は licence の表示を足す変更になる（3 つとも SIL OFL と記憶しているが NOT VERIFIED） | R10 が `THIRD_PARTY_NOTICES.md` を書くときに決める |
| W7 | 上限は 500 行で決めた（6.4.1）。描き直しを行ごとの差分にしたとき、上限をどこまで上げられるかは測っていない（NOT VERIFIED） | R6 が上限を 500 より上げたいときは、同じ測りで 1 行あたり 16 ms 以下を示して板へ戻す |
| W8 | 平均応答は平均のままで、p50 と p95 を出さない。主指標は p50 と p95（設計 3.1）で、画面の chip は今の頁と同じ値を出す | 出すかは設計の側が決める |
