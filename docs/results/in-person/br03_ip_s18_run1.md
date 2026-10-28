# BR-3 の IP-S18 の後の値（認識器の thread を 8 にした run）

設計 `docs/design/conversation_subtitles.ja.md` の 10.13.3（遅れの予算と測り方）と 10.13.7（受入 ID BR-3、T-BR-03-001）の線に対する値。合否は付けない。

## 段（BR-1。温まった標本は閉じる final を持つ発話の全部）

冷えた標本 2、温まった標本 76
d: n 76, p50 483 ms (線 300 ms) 線の外, p95 1061 ms (線 600 ms) 線の外
L_dec: n 76, p50 217 ms (線 270 ms) 線の内, p95 349 ms (線 480 ms) 線の内
L_tr: n 76, p50 74 ms (線 100 ms) 線の内, p95 345 ms (線 300 ms) 線の外
L_e: n 76, p50 239 ms, p95 419 ms
冷えた標本 repeat 1 utt 1 (ja): d 1697 ms, L_dec 198 ms, L_tr 363 ms
冷えた標本 repeat 1 utt 2 (en): d 189 ms, L_dec 271 ms, L_tr -1 ms

## 速報の訳 L_tr を言語で分けた値（BR-4。温まった標本）

L_tr ja: n 32, p50 174 ms, p95 380 ms
L_tr en: n 44, p50 8 ms, p95 272 ms

## 頁の段と頁までの遅れ（BR-2 の頁の時計と BR-1 を結んだ発話だけ）

結べた発話 78、結べなかった発話 0

L_pg: n 78, p50 1.300 ms (線 50 ms) 線の内, p95 3.700 ms (線 100 ms) 線の内
d: n 76, p50 483 ms (線 300 ms) 線の外, p95 1061 ms (線 600 ms) 線の外
L_dec: n 76, p50 217 ms (線 270 ms) 線の内, p95 349 ms (線 480 ms) 線の内
L_tr: n 76, p50 74 ms (線 100 ms) 線の内, p95 345 ms (線 300 ms) 線の外
L_page: n 76, p50 799 ms (線 750 ms) 線の外, p95 1353 ms (線 1300 ms) 線の外
L_settled_page: n 76, p50 1057 ms (線 1100 ms) 線の内, p95 1729 ms (線 1700 ms) 線の外
冷えた標本: n 2, `L_page` の最大 2262 ms (線 2500 ms) 線の内
