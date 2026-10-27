# BR-3 の IP-S17 の後の値（持った文を無音の中まで使う）

設計 `docs/design/conversation_subtitles.ja.md` の 10.13.3（遅れの予算と測り方）と 10.13.7（受入 ID BR-3、T-BR-03-001）の線に対する値。合否は付けない。

## 段（BR-1。温まった標本は閉じる final を持つ発話の全部）

冷えた標本 2、温まった標本 76
d: n 76, p50 514 ms (線 300 ms) 線の外, p95 1062 ms (線 600 ms) 線の外
L_dec: n 76, p50 211 ms (線 270 ms) 線の内, p95 473 ms (線 480 ms) 線の内
L_tr: n 76, p50 112 ms (線 100 ms) 線の外, p95 329 ms (線 300 ms) 線の外
L_e: n 76, p50 230 ms, p95 403 ms
冷えた標本 repeat 1 utt 1 (ja): d 3410 ms, L_dec 213 ms, L_tr 330 ms
冷えた標本 repeat 1 utt 2 (en): d 339 ms, L_dec 491 ms, L_tr 1 ms

## 速報の訳 L_tr を言語で分けた値（BR-4。温まった標本）

L_tr ja: n 32, p50 160 ms, p95 339 ms
L_tr en: n 44, p50 3 ms, p95 264 ms

## 頁の段と頁までの遅れ（BR-2 の頁の時計と BR-1 を結んだ発話だけ）

結べた発話 78、結べなかった発話 0

L_pg: n 78, p50 1.100 ms (線 50 ms) 線の内, p95 2.200 ms (線 100 ms) 線の内
d: n 76, p50 514 ms (線 300 ms) 線の外, p95 1062 ms (線 600 ms) 線の外
L_dec: n 76, p50 211 ms (線 270 ms) 線の内, p95 473 ms (線 480 ms) 線の内
L_tr: n 76, p50 112 ms (線 100 ms) 線の外, p95 329 ms (線 300 ms) 線の外
L_page: n 76, p50 873 ms (線 750 ms) 線の外, p95 1371 ms (線 1300 ms) 線の外
L_settled_page: n 76, p50 1124 ms (線 1100 ms) 線の外, p95 1689 ms (線 1700 ms) 線の内
冷えた標本: n 2, `L_page` の最大 3956 ms (線 2500 ms) 線の外
