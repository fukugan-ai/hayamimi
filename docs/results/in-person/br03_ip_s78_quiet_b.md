# BR-3 の IP-S7 だけの値（IP-S8 を旗で外した run）

設計 `docs/design/conversation_subtitles.ja.md` の 10.13.3（遅れの予算と測り方）と 10.13.7（受入 ID BR-3、T-BR-03-001）の線に対する値。合否は付けない。

## 段（BR-1。温まった標本は閉じる final を持つ発話の全部）

冷えた標本 2、温まった標本 76
d: n 76, p50 523 ms (線 300 ms) 線の外, p95 1062 ms (線 600 ms) 線の外
L_dec: n 76, p50 272 ms (線 270 ms) 線の外, p95 479 ms (線 480 ms) 線の内
L_tr: n 76, p50 22 ms (線 100 ms) 線の内, p95 311 ms (線 300 ms) 線の外
L_e: n 76, p50 226 ms, p95 364 ms
冷えた標本 repeat 1 utt 1 (ja): d 1920 ms, L_dec 208 ms, L_tr 315 ms
冷えた標本 repeat 1 utt 2 (en): d 345 ms, L_dec 491 ms, L_tr 3 ms

## 速報の訳 L_tr を言語で分けた値（BR-4。温まった標本）

L_tr ja: n 32, p50 143 ms, p95 345 ms
L_tr en: n 44, p50 3 ms, p95 245 ms

## 頁の段と頁までの遅れ（BR-2 の頁の時計と BR-1 を結んだ発話だけ）

結べた発話 78、結べなかった発話 0

L_pg: n 78, p50 1.200 ms (線 50 ms) 線の内, p95 2.500 ms (線 100 ms) 線の内
d: n 76, p50 523 ms (線 300 ms) 線の外, p95 1062 ms (線 600 ms) 線の外
L_dec: n 76, p50 272 ms (線 270 ms) 線の外, p95 479 ms (線 480 ms) 線の内
L_tr: n 76, p50 22 ms (線 100 ms) 線の内, p95 311 ms (線 300 ms) 線の外
L_page: n 76, p50 949 ms (線 750 ms) 線の外, p95 1618 ms (線 1300 ms) 線の外
L_settled_page: n 76, p50 1215 ms (線 1100 ms) 線の外, p95 1934 ms (線 1700 ms) 線の外
冷えた標本: n 2, `L_page` の最大 2445 ms (線 2500 ms) 線の内
