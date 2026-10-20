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

「早耳」と `hayamimi` を他の製品名に替えても、次の 4 つは成り立たない。
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
| shu | `#e04f2f` | 接続中の点、caret、logotype の「耳」 |
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
│ ● 早耳 hayamimi                     [確定 7] [平均応答 370 ms] [言語 en ko zh] │ header 約 61
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

## 12. 残る穴

| 番号 | 穴 | 持ち主と置き直す条件 |
|---|---|---|
| W1 | 速報の訳は `of` が `"partial"` の `translation` で見分ける（設計 3.6 の 3）。速報の訳の `id` は、後で確定文が取る `id` である。中継が同じ入力の直前の確定文の次の番号として決めるので、入力の最初の発話では速報の訳が出ない。画面は `of` が `"partial"` の訳を、同じ `id` の確定文の行とは別の行として描く | 持ち主は R6。速報の訳を描く 1 関数を adapter に置く |
| W3 | `fallback` の `stage` と `to` の値の一覧が、設計 3.15 に「段の名前」としか無い。11 の stage の表は値が決まった分だけ足す | R2 から R8c の席が値を決めるとき、表を足す |
| W6 | font を同梱しないので、入っていない端末では代替の字体で出る。同梱は licence の表示を足す変更になる（3 つとも SIL OFL と記憶しているが NOT VERIFIED） | R10 が `THIRD_PARTY_NOTICES.md` を書くときに決める |
| W7 | 上限は 500 行で決めた（6.4.1）。描き直しを行ごとの差分にしたとき、上限をどこまで上げられるかは測っていない（NOT VERIFIED） | R6 が上限を 500 より上げたいときは、同じ測りで 1 行あたり 16 ms 以下を示して板へ戻す |
| W8 | 平均応答は平均のままで、p50 と p95 を出さない。主指標は p50 と p95（設計 3.1）で、画面の chip は今の頁と同じ値を出す | 出すかは設計の側が決める |
